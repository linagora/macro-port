// SPDX-License-Identifier: Apache-2.0
//! The container indicators that extraction raises (`container_indicators` in
//! `rules/security.yaml`): their ids.
//!
//! The container layer has the ids of macro sheets and of a VBA project in a
//! package declared without macros (`OoxmlPackage::indicators`,
//! `container::XLM_MACROSHEET`). The two below are judged on the whole file.
//!
//! The ids stay apart from the report, in `Extraction::indicators`: the
//! security engine turns them into signals, once each, with the severity that
//! the rules give them (`engine::security`).

/// A failure with the status `suspicious_archive`: a compression anomaly.
pub(super) const SUSPICIOUS_ARCHIVE: &str = "sec.suspicious-archive";

/// A module whose source is suspected missing (SPEC M1, step 8).
pub(super) const SOURCE_MISSING: &str = "sec.source-missing";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::OoxmlPackage;
    use crate::report::Container;
    use crate::rules::{RuleSet, RulesSources};

    #[test]
    fn the_shipped_rules_list_every_indicator_that_extraction_raises() {
        let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
        let listed: Vec<&str> = (rules.security.container_indicators.iter())
            .map(|indicator| indicator.id.as_str())
            .collect();

        // A package with macro sheets and a VBA project, declared without macros.
        let package = OoxmlPackage {
            vba_project: Some(Vec::new()),
            macro_sheets: 1,
        };
        let of_the_container = package.indicators(Container::Xlsx);
        assert_eq!(of_the_container.len(), 2);

        let raised = of_the_container.into_iter();
        for id in raised.chain([SOURCE_MISSING, SUSPICIOUS_ARCHIVE]) {
            assert!(listed.contains(&id), "{id}");
        }
    }
}
