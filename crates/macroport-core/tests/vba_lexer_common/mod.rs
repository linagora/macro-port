// SPDX-License-Identifier: Apache-2.0
//! What the tokens of any text must be, whatever the text holds.

use macroport_core::vba::Token;

/// Whitespace for the lexer: what `char::is_whitespace` accepts, and U+0019.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{19}'
}

/// Why `tokens` do not cut `text` the way the lexer promises, if they do not:
/// in order, none empty or overlapping, each between two characters, and
/// nothing but whitespace left between them or around them.
pub fn tiling_problem(text: &str, tokens: &[Token]) -> Option<String> {
    let mut at = 0usize;
    for token in tokens {
        if token.start >= token.end || text.get(token.start..token.end).is_none() {
            return Some(format!("bad span {}..{}", token.start, token.end));
        }
        match text.get(at..token.start) {
            Some(gap) if gap.chars().all(is_space) => {}
            gap => return Some(format!("{gap:?} lies before the token at {}", token.start)),
        }
        at = token.end;
    }
    match text.get(at..) {
        Some(rest) if rest.chars().all(is_space) => None,
        rest => Some(format!("{rest:?} lies after the last token")),
    }
}
