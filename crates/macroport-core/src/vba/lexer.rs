// SPDX-License-Identifier: Apache-2.0
//! The VBA lexer (SPEC M2 item 2): the text of a logical line becomes tokens.
//!
//! The lexer only cuts the text. A [`Token`] names a kind and the bytes it
//! covers in the text it was cut from; only whitespace lies between two
//! tokens, so nothing is dropped, and no token holds a copy of the text. What
//! the text means is for the parser, and nothing in it is ever evaluated.

use super::scan::{
    bracket_len, date_literal_len, is_ident_char, is_ident_start, is_space, string_len,
};

/// Declares [`Keyword`] and the table of its spellings.
macro_rules! keywords {
    ($($variant:ident => $spelling:literal,)*) => {
        /// The words the parser reads the structure of a module from, and the
        /// operators that are spelled as words. The list is short and closed:
        /// every other word, `Len` and `Open` and `Shell` included, is an
        /// identifier, so that rules name it like any other.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Keyword {
            $(
                #[doc = concat!("The word `", $spelling, "`.")]
                $variant,
            )*
        }

        /// The keywords by lowercase spelling, in order for a binary search.
        const KEYWORDS: &[(&str, Keyword)] = &[$(($spelling, Keyword::$variant),)*];
    };
}

keywords! {
    AddressOf => "addressof",
    And => "and",
    As => "as",
    ByRef => "byref",
    ByVal => "byval",
    Call => "call",
    Case => "case",
    Const => "const",
    Declare => "declare",
    Dim => "dim",
    Do => "do",
    Each => "each",
    Else => "else",
    ElseIf => "elseif",
    End => "end",
    Enum => "enum",
    Eqv => "eqv",
    Event => "event",
    Exit => "exit",
    For => "for",
    Friend => "friend",
    Function => "function",
    Get => "get",
    Global => "global",
    GoSub => "gosub",
    GoTo => "goto",
    If => "if",
    Imp => "imp",
    Implements => "implements",
    In => "in",
    Is => "is",
    Let => "let",
    Like => "like",
    Loop => "loop",
    Mod => "mod",
    New => "new",
    Next => "next",
    Not => "not",
    On => "on",
    Option => "option",
    Optional => "optional",
    Or => "or",
    ParamArray => "paramarray",
    Preserve => "preserve",
    Private => "private",
    Property => "property",
    Public => "public",
    ReDim => "redim",
    Resume => "resume",
    Return => "return",
    Select => "select",
    Set => "set",
    Static => "static",
    Step => "step",
    Sub => "sub",
    Then => "then",
    To => "to",
    Type => "type",
    TypeOf => "typeof",
    Until => "until",
    Wend => "wend",
    While => "while",
    With => "with",
    WithEvents => "withevents",
    Xor => "xor",
}

/// The keyword spelled `word`, in any letter case.
fn keyword(word: &str) -> Option<Keyword> {
    let lowered = word.bytes().map(|byte| byte.to_ascii_lowercase());
    KEYWORDS
        .binary_search_by(|(spelling, _)| spelling.bytes().cmp(lowered.clone()))
        .ok()
        .and_then(|index| KEYWORDS.get(index))
        .map(|&(_, keyword)| keyword)
}

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A name: `Foo`, `Chr$` with its type character, or `[Sheet 1]`.
    Identifier,
    /// A word that gives the structure of the code.
    Keyword(Keyword),
    /// A string literal, quotes included. It runs to the end of the text when
    /// the closing quote is missing.
    String,
    /// A number: `42`, `1.5E3`, `10%`.
    Number,
    /// A date or time literal: `#1/2/2000#`.
    Date,
    /// `.`, which reaches a member: `Range.Value`, or `.Font` in a `With`.
    Dot,
    /// `!`, which reaches a member of a collection: `rs!Name`.
    Bang,
    /// `#` outside a date: `Print #1`, `#If`.
    Hash,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `:`, which only a lone colon outside the statement separators leaves.
    Colon,
    /// `:=`, which follows the name of a named argument.
    ColonEquals,
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `\`, the integer division.
    Backslash,
    /// `^`
    Caret,
    /// `&`, the concatenation.
    Ampersand,
    /// `=`, comparison or assignment.
    Equals,
    /// `<>` or `><`
    NotEqual,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `<=` or `=<`
    LessEqual,
    /// `>=` or `=>`
    GreaterEqual,
    /// Any other character.
    Unknown,
}

/// A token: its kind, and the bytes it covers in the text it was cut from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Offset of its first byte.
    pub start: usize,
    /// Offset after its last byte.
    pub end: usize,
}

/// The type-declaration characters of identifiers.
const TYPE_CHARACTERS: [char; 6] = ['$', '%', '&', '!', '#', '@'];

impl Token {
    /// The text of the token in `source`, the text it was cut from.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(self.start..self.end).unwrap_or_default()
    }

    /// The name an identifier stands for: without its brackets or its type
    /// character, so that `Chr$` and `Chr` are the same function. The text of
    /// any other token.
    pub fn name<'a>(&self, source: &'a str) -> &'a str {
        let text = self.text(source);
        if self.kind != TokenKind::Identifier {
            return text;
        }
        match text
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            Some(inside) => inside,
            None => text.strip_suffix(TYPE_CHARACTERS).unwrap_or(text),
        }
    }

    /// What a string literal holds, with its quotes off and `""` read as `"`.
    /// `None` for any other token.
    pub fn string_value(&self, source: &str) -> Option<String> {
        if self.kind != TokenKind::String {
            return None;
        }
        let text = self.text(source);
        let inside = text.strip_prefix('"')?;
        let closed = string_len(text) == Some(text.len());
        let inside = if closed {
            inside.strip_suffix('"').unwrap_or(inside)
        } else {
            inside
        };
        Some(inside.replace("\"\"", "\""))
    }
}

/// Reads the tokens of a text, from left to right.
pub struct Lexer<'a> {
    text: &'a str,
    at: usize,
    /// Whether the last token ends an operand: a name, a literal or `)`. A
    /// `.` that follows one reaches a member, and anywhere else it may start
    /// a number.
    after_operand: bool,
    /// Whether the last token is `.` or `!`, after which every word is a name.
    after_member: bool,
    /// Whether a `]` may still follow: false once a search has found none.
    brackets: bool,
}

impl<'a> Lexer<'a> {
    /// A lexer for `text`, the text of a logical line.
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            at: 0,
            after_operand: false,
            after_member: false,
            brackets: true,
        }
    }

    /// The kind and the length in bytes of the token that starts `rest`,
    /// which starts with `c`.
    fn read(&mut self, c: char, rest: &str) -> (TokenKind, usize) {
        let after = |len: usize| rest.get(len..).and_then(|text| text.chars().next());
        match c {
            '"' => (TokenKind::String, string_len(rest).unwrap_or(rest.len())),
            '[' if self.brackets => match bracket_len(rest) {
                Some(len) => (TokenKind::Identifier, len),
                None => {
                    self.brackets = false;
                    (TokenKind::Unknown, 1)
                }
            },
            _ if is_ident_start(c) || c == '_' => self.word(rest),
            '0'..='9' => (TokenKind::Number, number(rest)),
            '.' if !self.after_operand && after(1).is_some_and(|next| next.is_ascii_digit()) => {
                (TokenKind::Number, number(rest))
            }
            '.' => (TokenKind::Dot, 1),
            '!' => (TokenKind::Bang, 1),
            '#' => {
                date_literal_len(rest).map_or((TokenKind::Hash, 1), |len| (TokenKind::Date, len))
            }
            ',' => (TokenKind::Comma, 1),
            ';' => (TokenKind::Semicolon, 1),
            '(' => (TokenKind::LeftParen, 1),
            ')' => (TokenKind::RightParen, 1),
            '+' => (TokenKind::Plus, 1),
            '-' => (TokenKind::Minus, 1),
            '*' => (TokenKind::Star, 1),
            '/' => (TokenKind::Slash, 1),
            '\\' => (TokenKind::Backslash, 1),
            '^' => (TokenKind::Caret, 1),
            '&' => (TokenKind::Ampersand, 1),
            ':' if after(1) == Some('=') => (TokenKind::ColonEquals, 2),
            ':' => (TokenKind::Colon, 1),
            '=' => match after(1) {
                Some('<') => (TokenKind::LessEqual, 2),
                Some('>') => (TokenKind::GreaterEqual, 2),
                _ => (TokenKind::Equals, 1),
            },
            '<' => match after(1) {
                Some('>') => (TokenKind::NotEqual, 2),
                Some('=') => (TokenKind::LessEqual, 2),
                _ => (TokenKind::Less, 1),
            },
            '>' => match after(1) {
                Some('<') => (TokenKind::NotEqual, 2),
                Some('=') => (TokenKind::GreaterEqual, 2),
                _ => (TokenKind::Greater, 1),
            },
            _ => (TokenKind::Unknown, c.len_utf8()),
        }
    }

    /// The word that starts `rest`: a keyword, or an identifier with its type
    /// character. After `.` or `!`, and before `:=`, a word is always a name:
    /// `.End`, `Type:=8`.
    fn word(&self, rest: &str) -> (TokenKind, usize) {
        let end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
        let word = rest.get(..end).unwrap_or_default();
        let after = rest.get(end..).unwrap_or_default();
        let named_argument = after.trim_start_matches(is_space).starts_with(":=");
        if !self.after_member
            && !named_argument
            && let Some(keyword) = keyword(word)
        {
            return (TokenKind::Keyword(keyword), end);
        }
        (
            TokenKind::Identifier,
            end.saturating_add(type_character(after)),
        )
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let rest = self.text.get(self.at..)?;
        let gap = rest
            .len()
            .saturating_sub(rest.trim_start_matches(is_space).len());
        let start = self.at.saturating_add(gap);
        let rest = self.text.get(start..)?;
        let c = rest.chars().next()?;
        let (kind, len) = self.read(c, rest);
        let end = start.saturating_add(len);
        self.at = end;
        self.after_operand = matches!(
            kind,
            TokenKind::Identifier
                | TokenKind::Number
                | TokenKind::String
                | TokenKind::Date
                | TokenKind::RightParen
        );
        self.after_member = matches!(kind, TokenKind::Dot | TokenKind::Bang);
        Some(Token { kind, start, end })
    }
}

/// The tokens of `text`, the text of a logical line.
pub fn tokenize(text: &str) -> Vec<Token> {
    Lexer::new(text).collect()
}

/// Whether `c` can start an operand, after a character that might be a type
/// character or an operator.
fn starts_operand(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || matches!(c, '"' | '(' | '[' | '.' | '_')
}

/// Bytes of the type character that opens `after`, the text that follows a
/// name or a number: 0 or 1. `$`, `%` and `@` always are one. `!`, `&` and
/// `#` are when what follows could not be an operand or a file number:
/// `rs!Name` reaches a member, `a&b` joins strings and `Print#1` opens a file
/// number.
fn type_character(after: &str) -> usize {
    let mut chars = after.chars();
    let (Some(c), next) = (chars.next(), chars.next()) else {
        return 0;
    };
    let is_type_character = match c {
        '$' | '%' | '@' => true,
        '!' => !next.is_some_and(|n| is_ident_start(n) || n == '[' || n == '_'),
        '&' => !next.is_some_and(starts_operand),
        '#' => !next.is_some_and(|n| n.is_ascii_digit()),
        _ => false,
    };
    usize::from(is_type_character)
}

/// Bytes of the number that starts `rest`: digits, a fraction, an exponent
/// and a type character.
fn number(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let digits = |from: usize| {
        bytes.get(from..).map_or(0, |tail| {
            tail.iter().take_while(|byte| byte.is_ascii_digit()).count()
        })
    };
    let mut end = digits(0);
    if bytes.get(end) == Some(&b'.') {
        end = end
            .saturating_add(1)
            .saturating_add(digits(end.saturating_add(1)));
    }
    if matches!(bytes.get(end), Some(b'e' | b'E' | b'd' | b'D')) {
        let sign = usize::from(matches!(
            bytes.get(end.saturating_add(1)),
            Some(b'+' | b'-')
        ));
        let first = end.saturating_add(1).saturating_add(sign);
        let exponent = digits(first);
        if exponent > 0 {
            end = first.saturating_add(exponent);
        }
    }
    end.saturating_add(type_character(rest.get(end..).unwrap_or_default()))
}
