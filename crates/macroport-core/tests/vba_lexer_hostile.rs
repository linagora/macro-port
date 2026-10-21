// SPDX-License-Identifier: Apache-2.0
//! The lexer on hostile text (CLAUDE.md: every input is hostile). The tokens
//! of a text are capped, whatever its length.

mod vba_lexer_common;

use macroport_core::vba::{MAX_TOKENS, TokenKind, tokenize};
use vba_lexer_common::tiling_problem;

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
