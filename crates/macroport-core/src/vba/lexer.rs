// SPDX-License-Identifier: Apache-2.0
//! The VBA lexer (SPEC M2 item 2): the text of a logical line becomes tokens.
//!
//! The lexer only cuts the text. A [`Token`] names a kind and the bytes it
//! covers in the text it was cut from; only whitespace lies between two
//! tokens, so nothing is dropped, and no token holds a copy of the text. What
//! the text means is for the parser, and nothing in it is ever evaluated. The
//! physical line of a token is [`LogicalLine::line_at`] of its `start`.
//!
//! Lexing never fails: a character that starts no token is a
//! [`TokenKind::Unknown`] one. It takes time linear in the text, which is read
//! from left to right, and no lookahead that found nothing is made again.
//! [`tokenize`] is the way in; the lazy reader behind it stays private.
//!
//! [`LogicalLine::line_at`]: super::LogicalLine::line_at

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
        .map(|&(_, found)| found)
}

/// What a token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A name: `Foo`, `Chr$` with its type character, or `[Sheet 1]`. A word
    /// spelled like a keyword is a name after `.` or `!` (`.End`) and before
    /// `:=` (`Type:=8`).
    Identifier,
    /// A word that gives the structure of the code, in any letter case.
    Keyword(Keyword),
    /// A string literal, quotes included. It runs to the end of the text when
    /// the closing quote is missing, which VBA does not accept:
    /// [`Token::string_value`] is `None` for it.
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
    /// `:`. The logical lines are cut at the statement separators, so one
    /// has hardly any: it shows in a text that was not cut, and where a
    /// bracketed name runs over a continuation, which the line splitter reads
    /// one physical line at a time.
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
    /// A comment, from its `'`, or from the `Rem` that starts a statement, to
    /// the end of the text. The logical lines come without their comments,
    /// but a `Rem` after `Then` or `Else` is left in them, and the line then
    /// has no [`comment`](super::LogicalLine::comment) for it:
    /// [`Token::comment_text`] gives what it says. The parser skips the
    /// token.
    Comment,
    /// Any other character, or a `[` that no `]` follows.
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

    /// Whether the token is an identifier called `name`, in any ASCII letter
    /// case: the way to read the words that are not keywords, `Lib` and
    /// `Alias` of a `Declare` or the `Error` of `On Error`.
    pub fn is_name(&self, source: &str, name: &str) -> bool {
        self.kind == TokenKind::Identifier && self.name(source).eq_ignore_ascii_case(name)
    }

    /// What a string literal holds, with its quotes off and `""` read as `"`.
    /// `None` for any other token, and for a string whose closing quote is
    /// missing: it is not a literal, and nothing is made of it.
    pub fn string_value(&self, source: &str) -> Option<String> {
        if self.kind != TokenKind::String {
            return None;
        }
        let text = self.text(source);
        if string_len(text) != Some(text.len()) {
            return None;
        }
        let inside = text.strip_prefix('"')?.strip_suffix('"')?;
        Some(inside.replace("\"\"", "\""))
    }

    /// What a comment says, as [`Comment::text`](super::Comment::text) does
    /// for the comment a logical line was cut from: after its `'` or its
    /// `Rem`, trimmed. `None` for any other token.
    pub fn comment_text<'a>(&self, source: &'a str) -> Option<&'a str> {
        if self.kind != TokenKind::Comment {
            return None;
        }
        let text = self.text(source);
        // `Rem` is three ASCII bytes, so the cut falls on a character boundary.
        let said = text.strip_prefix('\'').or_else(|| text.get(3..))?;
        Some(said.trim_matches(is_space))
    }
}

/// Reads the tokens of a text, from left to right.
struct Lexer<'a> {
    text: &'a str,
    at: usize,
    /// The kind of the last token: the context of the next one, since the
    /// same characters mean different things after a name and after an
    /// operator.
    last: Option<TokenKind>,
    /// Whether a `]` may still follow: false once a search has found none.
    brackets: bool,
}

impl<'a> Lexer<'a> {
    /// A lexer for `text`, the text of a logical line.
    fn new(text: &'a str) -> Self {
        Self {
            text,
            at: 0,
            last: None,
            brackets: true,
        }
    }

    /// Whether the last token ends an operand: a name, a literal or `)`. A
    /// `.` that follows one reaches a member, and anywhere else it may start
    /// a number.
    fn after_operand(&self) -> bool {
        matches!(
            self.last,
            Some(
                TokenKind::Identifier
                    | TokenKind::Number
                    | TokenKind::String
                    | TokenKind::Date
                    | TokenKind::RightParen
            )
        )
    }

    /// Whether the last token is `.` or `!`, after which every word is a name.
    fn after_member(&self) -> bool {
        matches!(self.last, Some(TokenKind::Dot | TokenKind::Bang))
    }

    /// Whether a statement starts here: at the start of the text, after a
    /// `:`, and after the `Then` or `Else` of a single-line `If`.
    fn at_statement_start(&self) -> bool {
        matches!(
            self.last,
            None | Some(TokenKind::Colon | TokenKind::Keyword(Keyword::Then | Keyword::Else))
        )
    }

    /// The kind and the length in bytes of the token that starts `rest`, the
    /// text from its first character, which is not blank.
    fn read(&mut self, rest: &str) -> (TokenKind, usize) {
        let c = rest.chars().next().unwrap_or_default();
        let char_at = |len: usize| rest.get(len..).and_then(|tail| tail.chars().next());
        match c {
            '"' => (TokenKind::String, string_len(rest).unwrap_or(rest.len())),
            '\'' => (TokenKind::Comment, rest.len()),
            '[' if self.brackets => match bracket_len(rest) {
                Some(len) => (TokenKind::Identifier, len),
                None => {
                    self.brackets = false;
                    (TokenKind::Unknown, 1)
                }
            },
            _ if is_ident_start(c) => self.word(rest),
            '0'..='9' => (TokenKind::Number, number_len(rest)),
            '.' if !self.after_operand()
                && char_at(1).is_some_and(|next| next.is_ascii_digit()) =>
            {
                (TokenKind::Number, number_len(rest))
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
            ':' if char_at(1) == Some('=') => (TokenKind::ColonEquals, 2),
            ':' => (TokenKind::Colon, 1),
            '=' => match char_at(1) {
                Some('<') => (TokenKind::LessEqual, 2),
                Some('>') => (TokenKind::GreaterEqual, 2),
                _ => (TokenKind::Equals, 1),
            },
            '<' => match char_at(1) {
                Some('>') => (TokenKind::NotEqual, 2),
                Some('=') => (TokenKind::LessEqual, 2),
                _ => (TokenKind::Less, 1),
            },
            '>' => match char_at(1) {
                Some('<') => (TokenKind::NotEqual, 2),
                Some('=') => (TokenKind::GreaterEqual, 2),
                _ => (TokenKind::Greater, 1),
            },
            _ => (TokenKind::Unknown, c.len_utf8()),
        }
    }

    /// The word that starts `rest`: a keyword, a `Rem` comment, or an
    /// identifier with its type character. After `.` or `!`, and before `:=`,
    /// a word is always a name: `.End`, `Type:=8`.
    fn word(&self, rest: &str) -> (TokenKind, usize) {
        let end = rest.find(|c: char| !is_ident_char(c)).unwrap_or(rest.len());
        let word = rest.get(..end).unwrap_or_default();
        let tail = rest.get(end..).unwrap_or_default();
        if !self.after_member() {
            if self.at_statement_start() && word.eq_ignore_ascii_case("rem") {
                return (TokenKind::Comment, rest.len());
            }
            let named_argument = || tail.trim_start_matches(is_space).starts_with(":=");
            if let Some(found) = keyword(word)
                && !named_argument()
            {
                return (TokenKind::Keyword(found), end);
            }
        }
        (
            TokenKind::Identifier,
            end.saturating_add(type_character_len(tail)),
        )
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        let rest = self.text.get(self.at..)?.trim_start_matches(is_space);
        let first = rest.chars().next()?;
        // `rest` is the end of the text, so it starts where its length says.
        let start = self.text.len().saturating_sub(rest.len());
        let (kind, len) = self.read(rest);
        // Every token covers its first character: the lexer always advances.
        let end = start.saturating_add(len.max(first.len_utf8()));
        self.at = end;
        self.last = Some(kind);
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

/// Bytes of the type character that opens `tail`, the text that follows a
/// name or a number: 0 or 1. `$`, `%` and `@` always are one. `!`, `&` and
/// `#` are when what follows could not be an operand or a file number:
/// `rs!Name` reaches a member, `a&b` joins strings and `Print#1` opens a file
/// number.
fn type_character_len(tail: &str) -> usize {
    let mut chars = tail.chars();
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
fn number_len(rest: &str) -> usize {
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
    end.saturating_add(type_character_len(rest.get(end..).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyword_table_is_in_order_for_the_search() {
        for pair in KEYWORDS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (spelling, _) in KEYWORDS {
            assert!(
                spelling.bytes().all(|b| b.is_ascii_lowercase()),
                "{spelling}"
            );
        }
    }

    #[test]
    fn every_keyword_is_found_in_any_letter_case() {
        for &(spelling, expected) in KEYWORDS {
            let mut letters = spelling.chars();
            let title: String = letters
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(letters)
                .collect();
            let alternating: String = spelling
                .chars()
                .zip([true, false].into_iter().cycle())
                .map(|(c, upper)| if upper { c.to_ascii_uppercase() } else { c })
                .collect();
            for word in [spelling, &spelling.to_uppercase(), &title, &alternating] {
                assert_eq!(keyword(word), Some(expected), "{word}");
            }
        }
    }

    #[test]
    fn a_longer_word_a_shorter_word_or_another_script_is_no_keyword() {
        for word in ["", "i", "ifx", "xif", "iff", "elsei", "wit", "İf", "ıf"] {
            assert_eq!(keyword(word), None, "{word}");
        }
    }

    #[test]
    fn a_comment_says_what_follows_its_apostrophe_or_its_rem() {
        for (text, said) in [
            ("' a note ", "a note"),
            ("'", ""),
            ("Rem a note", "a note"),
            ("REM", ""),
            ("rem é", "é"),
        ] {
            let tokens = tokenize(text);
            assert_eq!(tokens.len(), 1, "{text:?}");
            assert_eq!(tokens[0].comment_text(text), Some(said), "{text:?}");
        }
    }

    #[test]
    fn the_rem_of_a_single_line_if_is_a_comment_the_line_does_not_hold() {
        let text = "If a Then Rem hi";
        let said: Vec<_> = tokenize(text)
            .iter()
            .filter_map(|token| token.comment_text(text))
            .collect();
        assert_eq!(said, ["hi"]);
    }

    #[test]
    fn only_a_comment_says_something() {
        let text = "x = \"a\" 'b";
        let said: Vec<_> = tokenize(text)
            .iter()
            .map(|token| token.comment_text(text))
            .collect();
        assert_eq!(said, [None, None, None, Some("b")]);
    }
}
