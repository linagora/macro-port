// SPDX-License-Identifier: Apache-2.0
//! The statements that cannot be read (SPEC M2 item 6). The parser builds no
//! tree of a statement, so it can only tell what no statement is, from its
//! tokens alone. The checks are the ones that valid code never fails, and a
//! statement that passes them all is not claimed to be valid: a false
//! alarm costs a procedure the trait `compile_error_suspected`, where a miss
//! costs nothing.

use crate::vba::expr::is_binary_operator;
use crate::vba::{Keyword, Token, TokenKind};

/// The offset in `text`, the logical line the `tokens` come from, of the first
/// token that keeps the statement from being read, or `None`. The first of the
/// five places that the checks below find, in the order of the text.
pub(super) fn unreadable(tokens: &[Token], text: &str) -> Option<usize> {
    let (first, last) = (tokens.first()?, tokens.last()?);
    [
        broken(tokens, text),
        unmatched(tokens),
        (!can_end(last.kind)).then_some(last.start),
        (!can_start(first.kind)).then_some(first.start),
        prose(tokens, text),
    ]
    .into_iter()
    .flatten()
    .min()
}

/// A character that is no token, or a string that never closes.
fn broken(tokens: &[Token], text: &str) -> Option<usize> {
    tokens
        .iter()
        .find(|token| match token.kind {
            TokenKind::Unknown => true,
            TokenKind::String => token.string_value(text).is_none(),
            _ => false,
        })
        .map(|token| token.start)
}

/// A `)` that closes nothing, or else the first `(` that nothing closes.
fn unmatched(tokens: &[Token]) -> Option<usize> {
    let (mut depth, mut outermost) = (0usize, 0usize);
    for token in tokens {
        match token.kind {
            TokenKind::LeftParen => {
                if depth == 0 {
                    outermost = token.start;
                }
                depth = depth.saturating_add(1);
            }
            TokenKind::RightParen if depth == 0 => return Some(token.start),
            TokenKind::RightParen => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    (depth > 0).then_some(outermost)
}

/// Whether a statement can end with a token of this kind. A binary operator has
/// no right operand, `Not` no operand at all, and a `.` or a `!` no member to
/// reach.
fn can_end(kind: TokenKind) -> bool {
    !(is_binary_operator(kind)
        || matches!(
            kind,
            TokenKind::Dot | TokenKind::Bang | TokenKind::Keyword(Keyword::Not)
        ))
}

/// Whether a statement can start with a token of this kind. The ones that
/// cannot are a binary operator, which needs a left operand (`-` is one, and no
/// statement starts with a sign), `Not`, a literal, the separators that sit
/// between two parts of a statement, and the words that only continue one:
/// `Then`, `To`, `In`, `Step`, `As`, `Each`, `Preserve`, the words of a
/// parameter, and the words that begin an operand (`New`, `AddressOf`,
/// `TypeOf`). A `)` is found by [`unmatched`], and a `:` never starts a logical
/// line: it splits them.
fn can_start(kind: TokenKind) -> bool {
    !(is_binary_operator(kind)
        || matches!(
            kind,
            TokenKind::Comma
                | TokenKind::Semicolon
                | TokenKind::ColonEquals
                | TokenKind::String
                | TokenKind::Number
                | TokenKind::Date
                | TokenKind::Keyword(
                    Keyword::Not
                        | Keyword::Then
                        | Keyword::To
                        | Keyword::In
                        | Keyword::Step
                        | Keyword::As
                        | Keyword::Each
                        | Keyword::Preserve
                        | Keyword::ByVal
                        | Keyword::ByRef
                        | Keyword::Optional
                        | Keyword::ParamArray
                        | Keyword::New
                        | Keyword::AddressOf
                        | Keyword::TypeOf
                )
        ))
}

/// The offset of a `To`, `Then`, `In` or `Step` outside parentheses in a
/// statement that starts with a name: those words belong to statements that
/// start with a keyword, and a line of prose such as `you need to import the
/// module in order to work` has them. `Lock` and `Unlock` take a `To`. Inside
/// parentheses, `To` is the bound of an array: `Items(1 To 10) As Long`.
fn prose(tokens: &[Token], text: &str) -> Option<usize> {
    let first = tokens.first()?;
    if first.kind != TokenKind::Identifier
        || first.is_name(text, "Lock")
        || first.is_name(text, "Unlock")
    {
        return None;
    }
    let mut depth = 0usize;
    for token in tokens {
        match token.kind {
            TokenKind::LeftParen => depth = depth.saturating_add(1),
            TokenKind::RightParen => depth = depth.saturating_sub(1),
            TokenKind::Keyword(Keyword::To | Keyword::Then | Keyword::In | Keyword::Step)
                if depth == 0 =>
            {
                return Some(token.start);
            }
            _ => {}
        }
    }
    None
}
