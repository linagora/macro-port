// SPDX-License-Identifier: Apache-2.0
//! The VBA lexer on arbitrary text: the text of a logical line, as the parser
//! gives it to `tokenize`. Bytes that are not UTF-8 are skipped, since the
//! sources reach the lexer decoded.

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::vba::{MAX_TOKENS, TokenKind, tokenize};

/// Whitespace as the lexer reads it, the only text allowed between tokens.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{19}'
}

fn only_spaces(text: &str, from: usize, to: usize) -> bool {
    text.get(from..to)
        .is_some_and(|gap| gap.chars().all(is_space))
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let tokens = tokenize(text);
    assert!(tokens.len() <= MAX_TOKENS);
    // The tokens cover the text in order, each a non-empty run of whole
    // characters, with only whitespace before, between and after them.
    let mut end = 0;
    for token in &tokens {
        assert!(token.start < token.end, "empty token {token:?}");
        assert!(
            text.get(token.start..token.end).is_some(),
            "{token:?} cuts a character"
        );
        assert!(
            only_spaces(text, end, token.start),
            "text dropped before {token:?}"
        );
        end = token.end;
        // The accessors never fail on a token of the text.
        let _ = token.name(text);
        let _ = token.string_value(text);
        let _ = token.comment_text(text);
    }
    assert!(
        only_spaces(text, end, text.len()),
        "text dropped at the end"
    );
    if tokens.len() == MAX_TOKENS {
        assert!(
            tokens
                .last()
                .is_some_and(|last| last.kind == TokenKind::Unknown)
        );
    }
    // Lexing is a function of the text alone.
    assert!(tokenize(text) == tokens);
});
