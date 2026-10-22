// SPDX-License-Identifier: Apache-2.0
//! The container indicators that extraction raises (`container_indicators` in
//! `rules/security.yaml`): their ids, and the file-scope signals they make in
//! a report.
//!
//! The container layer has the ids of macro sheets and of a VBA project in a
//! package declared without macros (`OoxmlPackage::indicators`,
//! `container::XLM_MACROSHEET`). The two below are judged on the whole file.
//!
//! The ids stay apart from the report, in `Extraction::indicators`: the
//! security engine of M3 turns them into signals, once each, and takes the
//! place of [`signals`] when it is wired in.

use crate::report::Signal;
use crate::rules::{RuleSet, Severity};

/// A failure with the status `suspicious_archive`: a compression anomaly.
pub(super) const SUSPICIOUS_ARCHIVE: &str = "sec.suspicious-archive";

/// A module whose source is suspected missing (SPEC M1, step 8).
pub(super) const SOURCE_MISSING: &str = "sec.source-missing";

/// The signals of the indicators among `raised`, in the order of the rules and
/// once each. An indicator that the rules do not list has no severity, so it
/// makes no signal. They are signals of the file: no module, no evidence.
pub(super) fn signals(rules: &RuleSet, raised: &[&str]) -> Vec<Signal> {
    (rules.security.container_indicators.iter())
        .filter(|indicator| raised.contains(&indicator.id.as_str()))
        .map(|indicator| Signal {
            signal_id: indicator.id.clone(),
            severity: indicator.severity,
            module: None,
            procedure: None,
            evidence: None,
        })
        .collect()
}

/// The highest severity among `signals`, if there is any.
pub(super) fn max_severity(signals: &[Signal]) -> Option<Severity> {
    signals.iter().map(|signal| signal.severity).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{OoxmlPackage, XLM_MACROSHEET};
    use crate::report::Container;
    use crate::rules::RulesSources;

    fn shipped_rules() -> RuleSet {
        RuleSet::load(&RulesSources::EMBEDDED).expect("the rules that ship with the tool load")
    }

    #[test]
    fn the_signals_follow_the_rules_and_come_once_each() {
        let raised = [
            XLM_MACROSHEET,
            SOURCE_MISSING,
            XLM_MACROSHEET,
            "sec.unknown",
        ];

        let signals = signals(&shipped_rules(), &raised);

        let found: Vec<_> = signals
            .iter()
            .map(|signal| (signal.signal_id.as_str(), signal.severity))
            .collect();
        let expected = [
            (SOURCE_MISSING, Severity::High),
            (XLM_MACROSHEET, Severity::Medium),
        ];
        assert_eq!(found, expected);
        let file_scope = |signal: &Signal| signal.module.is_none() && signal.evidence.is_none();
        assert!(signals.iter().all(file_scope));
    }

    #[test]
    fn the_shipped_rules_list_every_indicator_that_extraction_raises() {
        let rules = shipped_rules();
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

    #[test]
    fn the_worst_signal_gives_the_maximum_severity() {
        let rules = shipped_rules();

        assert_eq!(max_severity(&[]), None);
        let medium = signals(&rules, &[XLM_MACROSHEET]);
        assert_eq!(max_severity(&medium), Some(Severity::Medium));
        let both = signals(&rules, &[XLM_MACROSHEET, SOURCE_MISSING]);
        assert_eq!(max_severity(&both), Some(Severity::High));
    }
}
