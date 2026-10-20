// SPDX-License-Identifier: Apache-2.0
//! Physical lines and logical lines (SPEC M2 item 1).
//!
//! [`split_lines`] reads the text of one module as the file presents it. A
//! physical line keeps its number whatever happens to it. The statements
//! become logical lines, each carrying the physical line where it starts: `_`
//! continuations are joined, and comments are cut off and kept apart. Nothing
//! here evaluates anything: the text is only cut.

use super::VbaError;
use super::scan::{bracket_len, is_ident_char, is_space, string_len};
use crate::report::Status;

/// The most physical lines of a text that [`split_lines`] accepts.
///
/// Splitting costs memory per line, not per byte: a text made of one-character
/// lines takes about 90 times its size (measured), which against the 64 MB
/// SPEC M1 lets through for the VBA part would break its 256 MB per file. At
/// this limit the lines of a module take about 45 MB. The longest module of
/// the corpus has under 4,000 lines. The limit is per module: a caller that
/// splits several modules bounds their sum itself.
pub const MAX_LINES: u32 = 250_000;

/// The error of a text of more than [`MAX_LINES`] lines: the file is too
/// large to analyse, not corrupted.
fn too_many_lines() -> VbaError {
    VbaError::new(
        Status::TooLarge,
        "vba.too_many_lines",
        format!("more than {MAX_LINES} lines"),
    )
}

/// What a logical line holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalKind {
    /// Code: the statement, which the parser reads on.
    Code,
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
    /// The comment that ends the statement, if any.
    pub comment: Option<Comment>,
    /// Offset in `text` and physical line of each line after the first.
    breaks: Vec<(usize, u32)>,
}

impl LogicalLine {
    /// The physical line of byte `offset` of [`text`](Self::text), for the
    /// lexer: a continuation moves it to the line the text came from.
    pub fn line_at(&self, offset: usize) -> u32 {
        let passed = self.breaks.partition_point(|&(start, _)| start <= offset);
        passed
            .checked_sub(1)
            .and_then(|i| self.breaks.get(i))
            .map_or(self.line, |&(_, line)| line)
    }
}

/// The lines of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceLines {
    /// Number of physical lines of the text.
    pub physical_lines: u32,
    /// The statements and the comments between them, in order. Blank lines
    /// hold nothing.
    pub logical: Vec<LogicalLine>,
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
    };
    builder.body();
    if builder.too_many {
        return Err(too_many_lines());
    }
    builder.out.physical_lines = builder.consumed;
    Ok(builder.out)
}

/// The physical lines of a text, without their terminators.
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
}

impl<'a> Builder<'a> {
    /// The next physical line and its number. Nothing once the text is over
    /// [`MAX_LINES`] lines.
    fn next_line(&mut self) -> Option<(u32, &'a str)> {
        let line = self.rest.next()?;
        self.consumed = self.consumed.saturating_add(1);
        if self.consumed > MAX_LINES {
            self.too_many = true;
            return None;
        }
        Some((self.consumed, line))
    }

    /// Cuts the logical lines out of the physical lines of the text.
    fn body(&mut self) {
        while let Some((number, line)) = self.next_line() {
            if !line.trim_matches(is_space).is_empty() {
                self.chain(number, line);
            }
        }
    }

    /// Reads the physical lines of the statement that starts at `first` and
    /// turns them into a logical line.
    fn chain(&mut self, first: u32, line: &'a str) {
        let mut chain = Chain {
            code: String::new(),
            starts: Vec::new(),
            comment: None,
            last: first,
        };
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
                None => Self::read(&mut chain, number, line, &mut at_start),
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
        self.emit(chain);
    }

    /// Adds the code and the comment of a physical line to `chain`. True when
    /// the line is continued.
    fn read(chain: &mut Chain, number: u32, line: &str, at_start: &mut bool) -> bool {
        let scan = scan(line, at_start);
        let code = line.get(..scan.code_end).unwrap_or_default();
        let body = code.trim_matches(is_space);
        if !body.is_empty() {
            if !chain.code.is_empty() {
                chain.code.push(' ');
            }
            chain.starts.push((chain.code.len(), number));
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

    /// Pushes the logical line of `chain`: its code and its comment, or the
    /// comment alone. A chain with neither gives nothing.
    fn emit(&mut self, chain: Chain) {
        let Chain {
            code,
            starts,
            comment,
            last,
        } = chain;
        let (kind, line) = match (starts.first(), &comment) {
            (Some(&(_, line)), _) => (LogicalKind::Code, line),
            (None, Some(comment)) => (LogicalKind::CommentOnly, comment.line),
            (None, None) => return,
        };
        self.out.logical.push(LogicalLine {
            line,
            end_line: last,
            kind,
            text: code,
            comment,
            breaks: starts.into_iter().skip(1).collect(),
        });
    }
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

/// Reads `line` left to right: a comment ends the code, and a `_` ends the
/// line as a continuation unless it is inside a string. In a string that never
/// closes, the `_` is text and the next line a statement of its own: joined to
/// the unclosed string, its code would be read as text.
///
/// `at_start` tells whether the statement being read has no token yet, which
/// is where `Rem` starts a comment; it carries over to a continuation line,
/// the mark of which is no token.
fn scan(line: &str, at_start: &mut bool) -> Scan {
    let mark = marker(line);
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
        let start = is_space(c) && *at_start;
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
            '\'' => {
                comment = Some((at, at.saturating_add(1)));
                break;
            }
            'R' | 'r' if *at_start && is_rem(rest) => {
                comment = Some((at, at.saturating_add(3)));
                break;
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
