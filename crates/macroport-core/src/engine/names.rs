// SPDX-License-Identifier: Apache-2.0
//! How the matchers compare names (SPEC section 7): case-insensitively, and
//! without the type-declaration character that may end an identifier.

/// The characters that end an identifier to give it a type (SPEC M2 item 2).
const TYPE_CHARS: [char; 6] = ['$', '%', '&', '!', '#', '@'];

/// `name` without the type-declaration character that ends it: `Chr$` is
/// `Chr`.
pub(super) fn without_type_char(name: &str) -> &str {
    name.strip_suffix(TYPE_CHARS).unwrap_or(name)
}

/// A character in the form in which names are compared: in lower case, which
/// may take more than one character.
pub(super) fn fold_case(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase()
}

/// The characters of a name in the form in which names are compared.
fn comparable(name: &str) -> impl Iterator<Item = char> + '_ {
    without_type_char(name).chars().flat_map(fold_case)
}

/// Whether two names are the same name, whatever their case and the
/// type-declaration character that ends them. A name that is only a type
/// character is empty and is nothing to match.
///
/// This builds no string: it runs for every name of every line against every
/// name of every rule.
pub(super) fn same_name(left: &str, right: &str) -> bool {
    comparable(left).eq(comparable(right)) && comparable(left).next().is_some()
}

/// Whether `name` is the same name as one of `candidates`.
pub(super) fn is_one_of(name: &str, candidates: &[impl AsRef<str>]) -> bool {
    candidates
        .iter()
        .any(|candidate| same_name(name, candidate.as_ref()))
}

/// `name` in the form in which names are compared, as a string: two names are
/// the same name when their keys are equal and not empty. For the sets of
/// names, which look a name up and do not run through all of them.
pub(super) fn comparable_key(name: &str) -> String {
    comparable(name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_without_case() {
        assert!(same_name("Interior", "INTERIOR"));
        assert!(same_name("Génération", "GÉNÉRATION"));
        assert!(!same_name("Interior", "Interiors"));
        assert!(!same_name("Interior", "Exterior"));
    }

    #[test]
    fn a_type_character_ends_a_name_without_changing_it() {
        let typed = [
            ("Chr$", "Chr"),
            ("Mid%", "Mid"),
            ("Len&", "Len"),
            ("Fix!", "Fix"),
            ("Val#", "Val"),
            ("Cur@", "Cur"),
        ];
        for (with, without) in typed {
            assert!(same_name(with, without), "{with}");
            assert!(same_name(without, with), "{with}");
            assert!(same_name(with, &with.to_lowercase()), "{with}");
        }
        assert_eq!(TYPE_CHARS.len(), typed.len());
        // Only the last one goes.
        assert!(!same_name("a$b", "ab"));
        assert!(!same_name("a$$", "a"));
        assert!(!same_name("a$$", "a$"));
    }

    #[test]
    fn a_name_that_is_empty_matches_nothing() {
        assert!(!same_name("", ""));
        assert!(!same_name("$", ""));
        assert!(!same_name("$", "$"));
    }

    #[test]
    fn a_name_is_one_of_the_candidates_it_is_the_same_name_as() {
        assert!(is_one_of("shell$", &["Kill", "Shell"]));
        assert!(is_one_of("Kill", &["Kill".to_owned()]));
        assert!(!is_one_of("Shell", &["Kill", "Shells"]));
        assert!(!is_one_of("Shell", &[] as &[&str]));
        assert!(!is_one_of("$", &["", "$"]));
    }
}
