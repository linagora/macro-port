// SPDX-License-Identifier: Apache-2.0
//! The complexity level of a file (SPEC M3), from `rules/complexity.yaml`.
//!
//! The functions are pure. The metrics are those of the report, which the
//! caller has computed (SPEC M2 item 7): a metric condition bounds one of them,
//! and a metric that the report leaves out, such as `forms` for a procedure,
//! counts as 0.

use crate::report::{Level, Metrics};
use crate::rules::{Bound, Complexity, Metric, MetricCondition};

/// The complexity level of a file, from the metrics of the file: `High` when
/// any condition of `file_levels.high` holds, else `Medium` when any condition
/// of `file_levels.medium` does, else `Low`.
pub fn complexity_level(rules: &Complexity, file: &Metrics) -> Level {
    let reached = |conditions: &[MetricCondition]| {
        conditions
            .iter()
            .any(|condition| within_bound(file, condition))
    };
    if reached(&rules.file_levels.high) {
        Level::High
    } else if reached(&rules.file_levels.medium) {
        Level::Medium
    } else {
        Level::Low
    }
}

/// Whether the metric of `condition` is within its bound in `metrics`.
fn within_bound(metrics: &Metrics, condition: &MetricCondition) -> bool {
    let value = metric_value(metrics, condition.metric);
    match condition.bound {
        Bound::AtLeast(least) => value >= least,
        Bound::AtMost(most) => value <= most,
    }
}

/// The value of `metric` in `metrics`. A metric that the report leaves out
/// counts nothing, so it is 0.
fn metric_value(metrics: &Metrics, metric: Metric) -> u32 {
    match metric {
        Metric::Loc => metrics.loc,
        Metric::LogicalLoc => metrics.logical_loc,
        Metric::Procedures => metrics.procedures.unwrap_or_default(),
        Metric::Cyclomatic => metrics.cyclomatic.unwrap_or_default(),
        Metric::CyclomaticTotal => metrics.cyclomatic_total.unwrap_or_default(),
        Metric::MaxNesting => metrics.max_nesting.unwrap_or_default(),
        Metric::Parameters => metrics.parameters.unwrap_or_default(),
        Metric::GotoCount => metrics.goto_count.unwrap_or_default(),
        Metric::Forms => metrics.forms.unwrap_or_default(),
        Metric::DeclareCount => metrics.declare_count.unwrap_or_default(),
        Metric::ExternalDependencies => metrics.external_dependencies.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests;
