// SPDX-License-Identifier: Apache-2.0
//! Lexical pieces shared by the line splitter and the lexer: character
//! classes, the extent of string, bracket and date literals, and the reserved
//! identifiers. Every function reads a bounded prefix or a prefix it then
//! consumes, so a caller that skips what these return stays linear.

use std::sync::LazyLock;

use regex::Regex;

/// Whitespace of VBA source: what `char::is_whitespace` accepts (the Unicode
/// spaces, the tab, and also the form feed, the vertical tab and U+0085), plus
/// U+0019, which MS-VBAL lists as a whitespace character.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{19}'
}

/// A character that can start an identifier.
pub(crate) fn is_ident_start(c: char) -> bool {
    c.is_alphabetic()
}

/// A character that can continue an identifier.
pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Bytes of the line number that opens `statement`: ASCII digits that end at
/// the end of the text, at a blank or at the start of a name. `10 x = 1` and
/// `10x = 1` both open with the line number 10, while `10.5` and `10%` are
/// numbers.
pub(crate) fn line_number_len(statement: &str) -> Option<usize> {
    let digits = statement.bytes().take_while(u8::is_ascii_digit).count();
    let ends = statement
        .get(digits..)?
        .chars()
        .next()
        .is_none_or(|c| is_space(c) || is_ident_start(c));
    (digits > 0 && ends).then_some(digits)
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

/// The longest date or time literal, in characters after the opening `#`.
const DATE_LOOKAHEAD: usize = 48;

/// The shapes of the MS-VBAL date and time literals: a date of two or three
/// parts (numbers, or one month name), a time, or a date then a time. It
/// checks the shape, not the calendar.
fn date_pattern() -> String {
    let month = "jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?\
                 |sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?";
    let part = format!("(?:[0-9]{{1,4}}|{month})");
    let sep = r"(?:[ \t]+|[ \t]*[/,-][ \t]*)";
    let ampm = r"(?:[ \t]*(?:am|pm|a|p))";
    let clock = r"[0-9]{1,2}[ \t]*[:.][ \t]*[0-9]{1,2}(?:[ \t]*[:.][ \t]*[0-9]{1,2})?";
    let time = format!("(?:[0-9]{{1,2}}{ampm}|{clock}{ampm}?)");
    format!("(?i-u)^(?:{part}{sep}{part}(?:{sep}{part})?(?:[ \\t]+{time})?|{time})$")
}

static DATE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(&date_pattern()).ok());

/// Bytes of the date or time literal that starts `rest` (which starts with
/// `#`), both `#` included, or `None` when what follows is not one: `#1` of
/// `Print #1, x` and `#If` are not.
pub(crate) fn date_literal_len(rest: &str) -> Option<usize> {
    let body = rest.strip_prefix('#')?;
    let (close, _) = body
        .char_indices()
        .take(DATE_LOOKAHEAD)
        .find(|&(_, c)| c == '#')?;
    let inner = body.get(..close)?.trim_matches(is_space);
    DATE.as_ref()?
        .is_match(inner)
        .then(|| close.saturating_add(2))
}

/// The reserved identifiers of the MS-VBAL lexical grammar, lowercase, and
/// `randomize`, which a statement label may not be either. Statement labels
/// are the only use of the list so far.
const RESERVED: &str = "
    abs addressof and any array as attribute boolean byref byte byval call case cbool
    cbyte ccur cdate cdbl cdec cdecl cint circle clng clnglng clngptr close
    const csng cstr currency cvar cverr date debug decimal declare defbool
    defbyte defcur defdate defdbl defdec defint deflng deflnglng deflngptr
    defobj defsng defstr defvar dim do doevents double each else elseif empty
    end endif enum eqv erase error event exit false fix for friend function get
    global gosub goto if imp implements in input inputb int integer is
    lbound len lenb let like lineinput lock long longlong longptr loop lset me
    mod new next not nothing null on open option optional or paramarray
    preserve print private pset public put raiseevent randomize redim rem
    resume return rset scale seek select set sgn shared single spc static stop
    string sub tab then time to true type typeof ubound unlock until variant wend
    while with withevents write xor
    vb_base vb_control vb_creatable vb_customizable vb_description vb_exposed
    vb_ext_key vb_globalnamespace vb_helpid vb_invoke_func vb_invoke_property
    vb_invoke_propertyput vb_invoke_propertyputref vb_memberflags vb_name
    vb_predeclaredid vb_procdata vb_templatederived vb_usermemid
    vb_vardescription vb_varhelpid vb_varmemberflags vb_varprocdata
    vb_varusermemid";

/// Whether `word` is a reserved identifier, in any letter case.
pub(crate) fn is_reserved(word: &str) -> bool {
    RESERVED
        .split_ascii_whitespace()
        .any(|reserved| reserved.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_number_is_digits_that_a_blank_or_a_name_ends() {
        assert_eq!(line_number_len("10"), Some(2));
        assert_eq!(line_number_len("10 x"), Some(2));
        assert_eq!(line_number_len("10x"), Some(2));
        assert_eq!(line_number_len("007\t"), Some(3));
        for text in ["", "x", "10.5", "10%", "10:", "10_", "&H10", "1,2"] {
            assert_eq!(line_number_len(text), None, "{text}");
        }
    }

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

    #[test]
    fn dates_and_times_are_literals() {
        let accepted = [
            "#1/2/2000#",
            "#12/31/1999 11:59:59 PM#",
            "#2000-01-02#",
            "#Jan 1, 2000#",
            "#January 15 2000 3:30#",
            "#1:30 PM#",
            "#13:45:00#",
            "#9 am#",
            "#  1/2/2000  #",
            "#12.30#",
        ];
        for literal in accepted {
            let source = format!("{literal} + 1");
            assert_eq!(date_literal_len(&source), Some(literal.len()), "{literal}");
        }
    }

    #[test]
    fn a_literal_that_the_lookahead_cannot_reach_the_end_of_is_not_one() {
        let padded = |blanks: usize| format!("#{}1:30 PM# + 1", " ".repeat(blanks));
        assert_eq!(date_literal_len(&padded(10)), Some(19));
        assert_eq!(date_literal_len(&padded(60)), None);
    }

    #[test]
    fn file_numbers_and_directives_are_not_literals() {
        let refused = [
            "#1, x",
            "#1, 2: Close #1",
            "#1: Print #",
            "#If DEBUG Then",
            "#Const A = 1",
            "#1, #2",
            "# = 1 #",
            "#",
            "##",
            "#1/2/2000",
            "#1/2/2000 and more text than a literal can hold, so it is not one#",
        ];
        for text in refused {
            assert_eq!(date_literal_len(text), None, "{text}");
        }
    }

    #[test]
    fn reserved_identifiers_ignore_case_and_leave_library_names_alone() {
        for word in [
            "Else",
            "DOEVENTS",
            "randomize",
            "Rem",
            "VB_Name",
            "Date",
            "Error",
            "TIME",
        ] {
            assert!(is_reserved(word), "{word}");
        }
        for word in ["ErrorHandler", "Kill", "Shell", "Beep", "", "El"] {
            assert!(!is_reserved(word), "{word}");
        }
    }
}
