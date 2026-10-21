// SPDX-License-Identifier: Apache-2.0
//! The lexer on hostile text (CLAUDE.md: every input is hostile). The tokens
//! of a text are capped, whatever its length, and the work grows with the
//! length and no more: a lookahead that found nothing must not be made again
//! at the next character. Linear time shows on the clock only, and a limit in
//! seconds fails on a loaded machine before a slow algorithm does: each check
//! cuts a text and one a quarter of its length, and compares the two times
//! (`macroport_testkit::linear`). Random text made of the characters that mean
//! the most keeps its cut too.

mod vba_lexer_common;

use macroport_core::vba::{MAX_TOKENS, TokenKind, tokenize};
use macroport_testkit::linear::assert_linear_time;
use vba_lexer_common::tiling_problem;

/// The length of the hostile texts, in bytes.
const SIZE: usize = 1 << 18;

/// Cuts the text that `make` gives for `size` bytes, and the one it gives for a
/// quarter of `size`, and fails if cutting the first takes much more than four
/// times as long as cutting the second. Checks that the tokens of the first
/// tile it.
fn assert_cut_in_linear_time(what: &str, size: usize, make: impl Fn(usize) -> String) {
    let tokens = assert_linear_time(what, size, &make, |text| tokenize(text));
    assert_eq!(tiling_problem(&make(size), &tokens), None, "{what}");
}

/// `fragment` repeated until the text is `size` bytes long, and once more.
fn repeated(fragment: &str, size: usize) -> String {
    let copies = size.checked_div(fragment.len()).unwrap_or(1);
    fragment.repeat(copies.saturating_add(1))
}

#[test]
fn a_text_of_more_tokens_than_the_limit_ends_in_one_unknown_token() {
    for extra in [1, 10] {
        let text = "(".repeat(MAX_TOKENS.saturating_add(extra));
        let tokens = tokenize(&text);
        assert_eq!(tokens.len(), MAX_TOKENS, "{extra} more");
        let last = tokens.last().expect("a last token");
        assert_eq!(last.kind, TokenKind::Unknown, "{extra} more");
        assert_eq!(last.start, MAX_TOKENS.saturating_sub(1), "{extra} more");
        assert_eq!(last.end, text.len(), "{extra} more");
        assert!(
            tokens
                .iter()
                .take(MAX_TOKENS.saturating_sub(1))
                .all(|token| token.kind == TokenKind::LeftParen)
        );
        assert_eq!(tiling_problem(&text, &tokens), None, "{extra} more");
    }
}

#[test]
fn a_text_of_exactly_the_limit_is_cut_whole() {
    for tail in ["", "  \t"] {
        let text = format!("{}{tail}", "(".repeat(MAX_TOKENS));
        let tokens = tokenize(&text);
        assert_eq!(tokens.len(), MAX_TOKENS, "{tail:?}");
        let last = tokens.last().expect("a last token");
        assert_eq!(last.kind, TokenKind::LeftParen, "{tail:?}");
        assert_eq!(last.end, MAX_TOKENS, "{tail:?}");
        assert_eq!(tiling_problem(&text, &tokens), None, "{tail:?}");
    }
}

/// Each `[` looks for a `]` in the rest of the text: without the rule that a
/// search that fails is not made again, the time grows with the square of the
/// length.
#[test]
fn opening_brackets_that_nothing_closes_are_cut_in_linear_time() {
    for fragment in ["[", "[a", "[ ", "[[a"] {
        assert_cut_in_linear_time(&format!("{fragment:?}"), SIZE, |size| {
            repeated(fragment, size)
        });
    }
}

#[test]
fn a_repeated_fragment_is_cut_in_linear_time() {
    let fragments = [
        // Strings, comments and brackets that close: what reads on to a
        // closing character.
        "[a]",
        "]",
        "[]",
        "\"",
        "\"\"",
        "\"a",
        "\"a\"",
        "'",
        "Rem",
        "Rem ",
        ": Rem x",
        // Dates, file numbers, directives and type characters.
        "#",
        "# ",
        "#1 ",
        "#a,",
        "#1/",
        "#1/2/2000#",
        "#If ",
        "x#",
        "x#1",
        "x!",
        "x!y",
        "x$",
        "x%",
        "x&",
        "x@",
        // Hexadecimal and octal numbers, ampersands.
        "&",
        "&H",
        "&HF",
        "&HFFFFFFFF&",
        "&O",
        "&7",
        "&8",
        "a&",
        "&Hex(",
        "&H&H",
        "&&",
        // Numbers, dots and bangs.
        "1",
        "1.",
        "1e",
        "1e5",
        "1E+",
        "1D-",
        ".5",
        ".",
        "..",
        "!",
        "!.",
        "a.b!c",
        // Words, keywords and what separates them.
        "a",
        "é",
        "_",
        "End ",
        "If ",
        "Then Rem ",
        "Type:=",
        "Type :",
        "If   ",
        "\u{19}",
        "\t",
        " ",
        "\u{a0}",
        // Operators.
        "(",
        ")",
        ":=",
        ":",
        "<",
        ">",
        "<>",
        "=<",
        "=>",
        "><",
        "+",
        "-",
        "*",
        "/",
        "\\",
        "^",
        ",",
        ";",
    ];
    for fragment in fragments {
        assert_cut_in_linear_time(&format!("{fragment:?}"), SIZE, |size| {
            repeated(fragment, size)
        });
    }
}

#[test]
fn one_very_long_token_is_cut_in_linear_time() {
    // What stands before and after the long run of the middle character.
    let cases = [
        ("", "a", ""),
        ("", "1", ""),
        ("&H", "F", "g"),
        ("&H", "F", ""),
        ("&O", "7", "9"),
        ("\"", "a", ""),
        ("\"", "a", "\""),
        ("[", "a", ""),
        ("[", "a", "]"),
        ("'", "a", ""),
        ("If", " ", "x"),
        ("1.", "5", "e"),
        ("#", "1", ""),
        ("#", " ", "x"),
        ("#", " ", ""),
        ("x", "#", ""),
    ];
    for (before, middle, after) in cases {
        let what = format!("{before:?} {middle:?} {after:?}");
        assert_cut_in_linear_time(&what, SIZE, |size| {
            format!("{before}{}{after}", middle.repeat(size))
        });
    }
}

#[test]
fn many_keywords_and_hashes_each_followed_by_long_whitespace_are_cut_in_linear_time() {
    for word in ["If", "#"] {
        let piece = format!("{word}{}", " ".repeat(1_000));
        assert_cut_in_linear_time(&format!("{word:?} and spaces"), SIZE, |size| {
            piece.repeat(size.checked_div(piece.len()).unwrap_or(1))
        });
    }
}

/// A small deterministic generator (xorshift64*): the same texts on every
/// run, and nothing to import.
struct Random(u64);

impl Random {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0.wrapping_shr(12);
        self.0 ^= self.0.wrapping_shl(25);
        self.0 ^= self.0.wrapping_shr(27);
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `bound`, or 0 when there is none.
    fn below(&mut self, bound: usize) -> usize {
        let number = usize::try_from(self.step().wrapping_shr(16)).unwrap_or(0);
        number.checked_rem(bound).unwrap_or(0)
    }
}

/// The pieces of the random texts: the characters and words that decide where
/// a token stops, and some that start no token at all.
const PIECES: &[&str] = &[
    "a",
    "Z",
    "_",
    "é",
    "1",
    "9",
    "0",
    ".",
    "!",
    "#",
    "&",
    "H",
    "h",
    "O",
    "E",
    "e",
    "d",
    "$",
    "%",
    "@",
    "^",
    "[",
    "]",
    "(",
    ")",
    "\"",
    "'",
    ":",
    "=",
    "<",
    ">",
    "-",
    "+",
    "*",
    "/",
    "\\",
    ",",
    ";",
    " ",
    "\t",
    "\u{19}",
    "\u{a0}",
    "€",
    "Rem",
    "If",
    "Then",
    "Else",
    "&H",
    "&O17",
    "#1/2/2000#",
    "Write #1, 1, 2#",
    "x$",
    "Print ",
    "End ",
    ":=",
    "<>",
    "Type",
];

/// Whether the first character of a token is one its kind can start with.
fn starts_like(kind: TokenKind, text: &str) -> bool {
    let first = text.chars().next();
    match kind {
        TokenKind::String => first == Some('"'),
        TokenKind::Date => first == Some('#') && text.ends_with('#'),
        TokenKind::Number => first.is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '&'),
        TokenKind::Identifier => first.is_some_and(|c| c.is_alphabetic() || c == '['),
        TokenKind::Keyword(_) => first.is_some_and(char::is_alphabetic),
        TokenKind::Comment => {
            first == Some('\'')
                || text
                    .get(..3)
                    .is_some_and(|word| word.eq_ignore_ascii_case("rem"))
        }
        _ => true,
    }
}

#[test]
fn random_text_of_the_characters_that_matter_keeps_its_cut() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut seen: Vec<TokenKind> = Vec::new();
    for _ in 0..5_000 {
        let pieces = random.below(40);
        let text: String = (0..pieces)
            .filter_map(|_| PIECES.get(random.below(PIECES.len())))
            .copied()
            .collect();
        let tokens = tokenize(&text);
        assert_eq!(tiling_problem(&text, &tokens), None, "{text:?}");
        for token in &tokens {
            let part = token.text(&text);
            assert!(
                starts_like(token.kind, part),
                "{:?} in {text:?}",
                token.kind
            );
            if token.kind == TokenKind::Comment {
                assert_eq!(token.end, text.len(), "{text:?}");
            }
            if !seen.contains(&token.kind) {
                seen.push(token.kind);
            }
        }
    }
    // The checks above prove something only if the pieces reach the kinds
    // they look at.
    for kind in [
        TokenKind::Identifier,
        TokenKind::String,
        TokenKind::Number,
        TokenKind::Date,
        TokenKind::Hash,
        TokenKind::Comment,
        TokenKind::Unknown,
    ] {
        assert!(seen.contains(&kind), "{kind:?} never came up");
    }
    assert!(
        seen.iter()
            .any(|kind| matches!(kind, TokenKind::Keyword(_))),
        "no keyword came up"
    );
}
