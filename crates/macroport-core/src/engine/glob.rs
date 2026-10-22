// SPDX-License-Identifier: Apache-2.0
//! The glob patterns of the `progid` matcher.

use std::str::Chars;

/// Whether `text` matches the glob `pattern`, without regard to case. `*`
/// stands for any run of characters, the empty one included, `?` for any one
/// character, and every other character for itself: there are no character
/// classes, so `[` is a plain `[`. The whole text must match.
///
/// A hostile text cannot make the search long: it takes at most the product of
/// the two lengths in steps.
pub(super) fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut pattern_rest = pattern.chars();
    let mut text_rest = text.chars();
    // Where to go on when the match fails after a `*`: the pattern after the
    // star, and the text after the characters that the star has taken so far.
    let mut resume: Option<(Chars<'_>, Chars<'_>)> = None;
    loop {
        let mut after_pattern = pattern_rest.clone();
        match after_pattern.next() {
            Some('*') => {
                resume = Some((after_pattern.clone(), text_rest.clone()));
                pattern_rest = after_pattern;
                continue;
            }
            Some(wanted) => {
                let mut after_text = text_rest.clone();
                let found = after_text.next();
                if found.is_some_and(|found| wanted == '?' || same_char(wanted, found)) {
                    pattern_rest = after_pattern;
                    text_rest = after_text;
                    continue;
                }
            }
            None if text_rest.as_str().is_empty() => return true,
            None => {}
        }
        // The match failed: the last `*` takes one more character.
        let Some((star_pattern, star_text)) = resume.as_mut() else {
            return false;
        };
        if star_text.next().is_none() {
            return false;
        }
        pattern_rest = star_pattern.clone();
        text_rest = star_text.clone();
    }
}

/// Whether two characters are the same, apart from case.
fn same_char(left: char, right: char) -> bool {
    left == right || left.to_lowercase().eq(right.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::glob_matches;

    #[test]
    fn a_pattern_without_wildcard_is_the_whole_text_without_case() {
        assert!(glob_matches("Scripting.Dictionary", "scripting.DICTIONARY"));
        assert!(!glob_matches("Scripting", "Scripting.Dictionary"));
        assert!(!glob_matches("Dictionary", "Scripting.Dictionary"));
        assert!(!glob_matches("Scripting.Dictionary", "Scripting"));
    }

    #[test]
    fn a_star_takes_any_run_of_characters() {
        assert!(glob_matches("MSXML2.XMLHTTP*", "MSXML2.XMLHTTP"));
        assert!(glob_matches("MSXML2.XMLHTTP*", "MSXML2.XMLHTTP.6.0"));
        assert!(glob_matches("msxml2.xmlhttp*", "MSXML2.XMLHTTP60"));
        assert!(!glob_matches("MSXML2.XMLHTTP*", "MSXML2.DOMDocument"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*", "Any.Thing"));
        assert!(glob_matches("*.Application", "Word.Application"));
    }

    #[test]
    fn a_star_goes_back_to_try_a_shorter_run() {
        assert!(glob_matches("a*b*c", "abc"));
        assert!(glob_matches("a*b*c", "aXXbYYbZc"));
        assert!(glob_matches("a**c", "ac"));
        assert!(!glob_matches("a*b*c", "aXbX"));
        assert!(!glob_matches("a*b*c", "ac"));
    }

    #[test]
    fn a_question_mark_takes_one_character() {
        assert!(glob_matches("a?c", "abc"));
        assert!(glob_matches("a?c", "a\u{1D11E}c"));
        assert!(!glob_matches("a?c", "ac"));
        assert!(!glob_matches("a?c", "abbc"));
    }

    #[test]
    fn the_empty_pattern_matches_the_empty_text_only() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "a"));
    }

    #[test]
    fn case_follows_unicode() {
        assert!(glob_matches("\u{E9}*", "\u{C9}COLE"));
    }

    #[test]
    fn a_failing_search_ends() {
        let text = "a".repeat(2000);
        assert!(!glob_matches("*a*a*a*a*a*b", &text));
    }
}
