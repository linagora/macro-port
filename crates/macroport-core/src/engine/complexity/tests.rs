// SPDX-License-Identifier: Apache-2.0
//! Tests of the complexity levels.

use super::*;
use crate::rules::{RuleSet, RulesSources};

/// The rules shipped in `rules/`.
fn shipped() -> RuleSet {
    RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
}

/// Every metric, defined: the conditions of the tests name any of them.
const DEFINITIONS: &str = "{ loc: X, logical_loc: X, procedures: X, cyclomatic: X, \
                           cyclomatic_total: X, max_nesting: X, parameters: X, goto_count: X, \
                           forms: X, declare_count: X, external_dependencies: X }";

/// A `complexity.yaml` whose medium and high levels have these conditions and
/// whose traits are these (an id and the rule of the trait), loaded beside the
/// shipped rules.
fn complexity(medium: &str, high: &str, traits: &[(&str, &str)]) -> Complexity {
    let items: Vec<String> = traits
        .iter()
        .map(|(id, rule)| {
            format!("{{ id: {id}, label: {{ en: X, fr: X }}, description: X, {rule} }}")
        })
        .collect();
    let text = format!(
        "version: {}\n\
         file_levels: {{ medium: {{ any: [{medium}] }}, high: {{ any: [{high}] }} }}\n\
         metric_definitions: {DEFINITIONS}\n\
         traits: [{}]\n",
        shipped().version(),
        items.join(", ")
    );
    let sources = RulesSources {
        complexity: &text,
        ..RulesSources::EMBEDDED
    };
    RuleSet::load(&sources).expect("the rules load").complexity
}

/// Metrics of this many logical lines, and none of the optional metrics.
fn metrics(logical_loc: u32) -> Metrics {
    Metrics {
        loc: logical_loc,
        logical_loc,
        procedures: None,
        cyclomatic: None,
        cyclomatic_total: None,
        max_nesting: None,
        parameters: None,
        goto_count: None,
        forms: None,
        declare_count: None,
        external_dependencies: None,
    }
}

#[test]
fn a_file_is_low_until_a_condition_of_a_level_holds() {
    let rules = complexity(
        "{ metric: logical_loc, gte: 100 }, { metric: procedures, gte: 5 }",
        "{ metric: logical_loc, gte: 1000 }, { metric: forms, gte: 3 }",
        &[],
    );
    let level = |file: Metrics| complexity_level(&rules, &file);

    // A bound holds from its value.
    assert_eq!(level(metrics(99)), Level::Low);
    assert_eq!(level(metrics(100)), Level::Medium);
    assert_eq!(level(metrics(999)), Level::Medium);
    assert_eq!(level(metrics(1000)), Level::High);
    // One condition is enough, whichever metric it bounds.
    let procedures = |count| Metrics {
        procedures: Some(count),
        ..metrics(0)
    };
    assert_eq!(level(procedures(4)), Level::Low);
    assert_eq!(level(procedures(5)), Level::Medium);
    let forms = |count| Metrics {
        forms: Some(count),
        ..metrics(0)
    };
    assert_eq!(level(forms(2)), Level::Low);
    assert_eq!(level(forms(3)), Level::High);
    // The highest level that is reached wins.
    let both = Metrics {
        procedures: Some(5),
        forms: Some(3),
        ..metrics(100)
    };
    assert_eq!(level(both), Level::High);
}

#[test]
fn an_upper_bound_holds_up_to_its_value_and_a_missing_metric_is_zero() {
    let rules = complexity(
        "{ metric: procedures, lte: 2 }",
        "{ metric: forms, gte: 1 }",
        &[],
    );
    let level = |file: Metrics| complexity_level(&rules, &file);
    let procedures = |count| Metrics {
        procedures: Some(count),
        ..metrics(0)
    };

    assert_eq!(level(procedures(2)), Level::Medium);
    assert_eq!(level(procedures(3)), Level::Low);
    // The metrics are counts, and a count of nothing is left out of the report.
    assert_eq!(level(metrics(0)), Level::Medium);
    let one_form = Metrics {
        forms: Some(1),
        ..metrics(0)
    };
    assert_eq!(level(one_form), Level::High);
}

#[test]
fn every_metric_reads_the_field_of_the_report_that_bears_its_name() {
    let report = Metrics {
        loc: 1,
        logical_loc: 2,
        procedures: Some(3),
        cyclomatic: Some(4),
        cyclomatic_total: Some(5),
        max_nesting: Some(6),
        parameters: Some(7),
        goto_count: Some(8),
        forms: Some(9),
        declare_count: Some(10),
        external_dependencies: Some(11),
    };
    let written = serde_json::to_value(&report).expect("the metrics serialise");

    for metric in Metric::ALL {
        let in_report = written[metric.name()].as_u64();
        assert_eq!(
            Some(u64::from(metric_value(&report, metric))),
            in_report,
            "{}",
            metric.name()
        );
    }
}

#[test]
fn a_metric_left_out_of_the_report_is_zero() {
    for metric in Metric::ALL {
        assert_eq!(metric_value(&metrics(0), metric), 0, "{}", metric.name());
    }
}

#[test]
fn the_shipped_levels_read_the_cyclomatic_metric_of_the_report() {
    let rules = shipped().complexity;
    let level = |file: Metrics| complexity_level(&rules, &file);

    assert_eq!(level(metrics(20)), Level::Low);
    assert_eq!(level(metrics(100_000)), Level::High);
    // The highest cyclomatic complexity of the file is `cyclomatic` (#29).
    let complicated = Metrics {
        cyclomatic: Some(1000),
        ..metrics(20)
    };
    assert_eq!(level(complicated), Level::High);
}
