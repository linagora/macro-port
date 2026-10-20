// SPDX-License-Identifier: Apache-2.0
//! Lexical pieces shared by the line splitter and the lexer: character
//! classes and the extent of string and bracket literals. Every function
//! reads a bounded prefix or a prefix it then consumes, so a caller that
//! skips what these return stays linear.

/// Whitespace of VBA source: what `char::is_whitespace` accepts (the Unicode
/// spaces, the tab, and also the form feed, the vertical tab and U+0085), plus
/// U+0019, which MS-VBAL lists as a whitespace character.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{19}'
}

/// A character that can continue an identifier.
pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Bytes of the string literal that starts `rest` (which starts with `"`),
/// closing quote included, or `None` when it is not closed: it then reaches
/// the end of `rest`. A doubled quote inside is a quote.
pub(crate) fn string_len(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c != '"' {
            continue;
        }
        if chars.next_if(|&(_, next)| next == '"').is_none() {
            return Some(i.saturating_add(1));
        }
    }
    None
}

/// Bytes of the `[bracketed name]` that starts `rest` (which starts with
/// `[`), or `None` when no `]` follows. The search for the `]` reads the
/// whole of `rest` when there is none, so a caller stops looking for
/// brackets in a line after the first `None`.
pub(crate) fn bracket_len(rest: &str) -> Option<usize> {
    rest.get(1..)?.find(']')?.checked_add(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_ends_at_its_closing_quote_and_doubled_quotes_do_not_close_it() {
        assert_eq!(string_len(r#""abc" + 1"#), Some(5));
        assert_eq!(string_len(r#""say ""hi"" now" & x"#), Some(16));
        assert_eq!(string_len(r#""""#), Some(2));
        assert_eq!(string_len(r#""""""#), Some(4));
        assert_eq!(string_len("\"é\" 1"), Some(4));
    }

    #[test]
    fn a_string_that_is_not_closed_has_no_length() {
        assert_eq!(string_len("\"abc"), None);
        assert_eq!(string_len("\"abc\"\""), None);
        assert_eq!(string_len("\""), None);
    }

    #[test]
    fn a_bracketed_name_ends_at_the_first_closing_bracket() {
        assert_eq!(bracket_len("[Sheet 1].Range"), Some(9));
        assert_eq!(bracket_len("[a:b']"), Some(6));
        assert_eq!(bracket_len("[]"), Some(2));
        assert_eq!(bracket_len("[abc"), None);
        assert_eq!(bracket_len("["), None);
    }
}
