// SPDX-License-Identifier: Apache-2.0
//! What a matcher found: where, and the text to quote.

use crate::report::Evidence;

/// The longest snippet of an evidence, in characters: the `maxLength` of the
/// report schema.
pub const MAX_SNIPPET_CHARS: usize = 200;

/// A line of the source of a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    /// Index of the module in `ProjectInput::modules`.
    pub module: usize,
    /// The 1-based physical line in the module source as the file presents it
    /// (SPEC section 5).
    pub line: u32,
}

/// One place where a matcher matched.
///
/// A hit borrows its text from the project that was matched, so that counting
/// hits costs no allocation: only the hits that are kept need a [`snippet`].
///
/// [`snippet`]: Hit::snippet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit<'p> {
    /// Where the match is, or `None` for a hit that is not in module source:
    /// a reference of the project. A hit about a whole module, such as a
    /// `module_type` hit, cites the first line of the module.
    pub place: Option<Place>,
    /// What the snippet quotes: the statement that matched, the declaration
    /// of the procedure, the name of the module or of the reference.
    text: &'p str,
}

impl<'p> Hit<'p> {
    /// A hit at `place` that quotes `text`.
    pub fn new(place: Option<Place>, text: &'p str) -> Self {
        Self { place, text }
    }

    /// The text to quote as evidence: the quoted text without the white space
    /// around it, cut to [`MAX_SNIPPET_CHARS`] characters on a character
    /// boundary.
    pub fn snippet(&self) -> String {
        self.text.trim().chars().take(MAX_SNIPPET_CHARS).collect()
    }

    /// The evidence of the rule `rule_id` for this hit, or `None` when the hit
    /// has no line to cite: the caller then cites the line of its choice, with
    /// [`snippet`](Hit::snippet). A line below 1 is cited as line 1, the
    /// least the report schema accepts, so that the report stays valid
    /// whatever the parser gave.
    pub fn evidence(&self, rule_id: &str) -> Option<Evidence> {
        let place = self.place?;
        Some(Evidence {
            rule_id: rule_id.to_owned(),
            line: place.line.max(1),
            snippet: Some(self.snippet()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32) -> Option<Place> {
        Some(Place { module: 2, line })
    }

    #[test]
    fn snippet_drops_the_white_space_around_the_text() {
        let hit = Hit::new(at(5), "\t    Range(\"A1\").Select \t");
        assert_eq!(hit.snippet(), "Range(\"A1\").Select");
    }

    #[test]
    fn snippet_keeps_text_up_to_the_limit_whole() {
        assert_eq!(Hit::new(at(5), "Kill path").snippet(), "Kill path");
        let text = "a".repeat(MAX_SNIPPET_CHARS);
        assert_eq!(Hit::new(at(5), &text).snippet(), text);
    }

    #[test]
    fn snippet_is_cut_to_the_limit() {
        let text = "x".repeat(MAX_SNIPPET_CHARS + 50);
        let snippet = Hit::new(at(5), &text).snippet();
        assert_eq!(snippet, "x".repeat(MAX_SNIPPET_CHARS));
    }

    #[test]
    fn snippet_counts_characters_not_bytes() {
        // 3-byte characters: a cut at byte 200 would fall inside the 67th one.
        let text = "€".repeat(MAX_SNIPPET_CHARS + 1);
        assert_eq!(
            Hit::new(at(5), &text).snippet(),
            "€".repeat(MAX_SNIPPET_CHARS)
        );
        // 4-byte characters, outside the basic plane.
        let text = "𝄞".repeat(MAX_SNIPPET_CHARS + 1);
        assert_eq!(
            Hit::new(at(5), &text).snippet(),
            "𝄞".repeat(MAX_SNIPPET_CHARS)
        );
    }

    #[test]
    fn snippet_of_white_space_is_empty() {
        assert_eq!(Hit::new(at(5), " \t ").snippet(), "");
    }

    #[test]
    fn evidence_cites_the_rule_the_line_and_the_snippet() {
        let hit = Hit::new(at(12), "  Kill path  ");
        assert_eq!(
            hit.evidence("sec.file-delete"),
            Some(Evidence {
                rule_id: "sec.file-delete".to_owned(),
                line: 12,
                snippet: Some("Kill path".to_owned()),
            })
        );
    }

    #[test]
    fn evidence_never_cites_a_line_below_one() {
        let hit = Hit::new(at(0), "Kill path");
        assert_eq!(hit.evidence("sec.file-delete").map(|e| e.line), Some(1));
    }

    #[test]
    fn the_snippet_limit_is_the_max_length_of_the_report_schema() {
        const SCHEMA: &str = include_str!("../../../../schema/report.schema.json");
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let max_length = &schema["$defs"]["evidence"]["properties"]["snippet"]["maxLength"];
        assert_eq!(max_length.as_u64(), u64::try_from(MAX_SNIPPET_CHARS).ok());
    }

    #[test]
    fn a_hit_without_a_line_has_no_evidence() {
        let hit = Hit::new(None, "Microsoft Outlook 16.0 Object Library");
        assert_eq!(hit.evidence("office.references"), None);
        assert_eq!(hit.snippet(), "Microsoft Outlook 16.0 Object Library");
    }

    #[test]
    fn places_order_by_module_then_line() {
        let mut places = [
            Place { module: 1, line: 3 },
            Place { module: 0, line: 9 },
            Place { module: 1, line: 1 },
        ];
        places.sort();
        assert_eq!(
            places,
            [
                Place { module: 0, line: 9 },
                Place { module: 1, line: 1 },
                Place { module: 1, line: 3 },
            ]
        );
    }
}
