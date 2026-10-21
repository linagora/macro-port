// SPDX-License-Identifier: Apache-2.0
//! The lexer on hostile text (CLAUDE.md: every input is hostile). The tokens
//! of a text are capped, whatever its length, and the work grows with the
//! length and no more: a lookahead that found nothing must not be made again
//! at the next character. Linear time shows on the clock only, so the limit is
//! set far above what a single pass takes and far below what a reading of the
//! rest of the text at every character would take on these sizes.

// clippy.toml keeps the clock out of the core, not out of its tests.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod vba_lexer_common;

use std::time::{Duration, Instant};

use macroport_core::vba::{MAX_TOKENS, TokenKind, tokenize};
use vba_lexer_common::tiling_problem;

/// The length of the hostile texts, unless a test says otherwise.
const SIZE: usize = 1 << 18;

/// What cutting one text may take.
const LIMIT: Duration = Duration::from_secs(5);

/// Cuts `text`, fails if that takes longer than [`LIMIT`] and checks that the
/// tokens tile the text.
fn assert_cut_in_time(text: &str) {
    let started = Instant::now();
    let tokens = tokenize(text);
    let elapsed = started.elapsed();
    let head: String = text.chars().take(40).collect();
    assert!(elapsed < LIMIT, "{head:?} took {elapsed:?}");
    assert_eq!(tiling_problem(text, &tokens), None, "{head:?}");
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
/// search that fails is not made again, 2 MB of them take about a minute.
#[test]
fn opening_brackets_that_nothing_closes_are_cut_in_linear_time() {
    for fragment in ["[", "[a", "[ ", "[[a"] {
        let copies = (2usize << 20).checked_div(fragment.len()).unwrap_or(1);
        assert_cut_in_time(&fragment.repeat(copies));
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
        let copies = SIZE.checked_div(fragment.len()).unwrap_or(1);
        assert_cut_in_time(&fragment.repeat(copies.saturating_add(1)));
    }
}

#[test]
fn one_very_long_token_is_cut_in_linear_time() {
    let long =
        |before: &str, middle: &str, after: &str| format!("{before}{}{after}", middle.repeat(SIZE));
    let texts = [
        long("", "a", ""),
        long("", "1", ""),
        long("&H", "F", "g"),
        long("&H", "F", ""),
        long("&O", "7", "9"),
        long("\"", "a", ""),
        long("\"", "a", "\""),
        long("[", "a", ""),
        long("[", "a", "]"),
        long("'", "a", ""),
        long("If", " ", "x"),
        long("1.", "5", "e"),
        long("#", "1", ""),
        long("#", " ", "x"),
        long("#", " ", ""),
        long("x", "#", ""),
    ];
    for text in &texts {
        assert_cut_in_time(text);
    }
}

#[test]
fn many_keywords_and_hashes_each_followed_by_long_whitespace_are_cut_in_linear_time() {
    for word in ["If", "#"] {
        let piece = format!("{word}{}", " ".repeat(1_000));
        assert_cut_in_time(&piece.repeat(SIZE.checked_div(piece.len()).unwrap_or(1)));
    }
}
