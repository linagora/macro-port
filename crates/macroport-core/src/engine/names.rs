// SPDX-License-Identifier: Apache-2.0
//! How the matchers compare names (SPEC section 7): case-insensitively, and
//! without the type-declaration character that may end an identifier.

use std::collections::BTreeSet;

/// The characters that end an identifier to give it a type (SPEC M2 item 2).
const TYPE_CHARS: [char; 6] = ['$', '%', '&', '!', '#', '@'];

/// `name` without the type-declaration character that ends it: `Chr$` is
/// `Chr`.
pub(super) fn without_type_char(name: &str) -> &str {
    name.strip_suffix(TYPE_CHARS).unwrap_or(name)
}

/// The form in which two names are compared: lowercase, without brackets and
/// without the type-declaration character.
pub(super) fn fold_name(name: &str) -> String {
    let unbracketed = name
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(name);
    without_type_char(unbracketed).to_lowercase()
}

/// The names of a rule, folded to be looked up.
pub(super) fn fold_all(names: &[String]) -> BTreeSet<String> {
    names.iter().map(|name| fold_name(name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_without_case() {
        assert_eq!(fold_name("Interior"), fold_name("INTERIOR"));
        assert_eq!(fold_name("Génération"), fold_name("GÉNÉRATION"));
    }

    #[test]
    fn a_type_character_ends_a_name_without_changing_it() {
        for typed in ["Chr$", "Mid%", "Len&", "Fix!", "Val#", "Cur@"] {
            let bare = typed.trim_end_matches(TYPE_CHARS);
            assert_eq!(fold_name(typed), fold_name(bare), "{typed}");
        }
        assert_eq!(fold_name("Chr$"), "chr");
    }

    #[test]
    fn only_the_last_type_character_goes() {
        assert_eq!(fold_name("a$b"), "a$b");
        assert_eq!(fold_name("$"), "");
    }

    #[test]
    fn brackets_around_a_name_go() {
        assert_eq!(fold_name("[Shell]"), "shell");
        assert_eq!(fold_name("[Sheet 1]"), "sheet 1");
        assert_eq!(fold_name("[Shell"), "[shell");
    }

    #[test]
    fn rule_names_fold_into_a_set() {
        let names = [
            "Interior".to_owned(),
            "BORDERS".to_owned(),
            "interior".to_owned(),
        ];
        let folded: Vec<_> = fold_all(&names).into_iter().collect();
        assert_eq!(folded, ["borders", "interior"]);
    }
}
