// SPDX-License-Identifier: Apache-2.0
//! The complexity level of a file and the traits of its procedures (SPEC M3),
//! from `rules/complexity.yaml`.
//!
//! The functions are pure. The metrics are those of the report, which the
//! caller has computed (SPEC M2 item 7): a metric condition bounds one of them,
//! and a metric that the report leaves out, such as `forms` for a procedure,
//! counts as 0. The other conditions read the source through an [`Evaluator`],
//! as the signals do.

use super::eval::Evaluator;
use crate::report::{Level, Metrics};
use crate::rules::{
    Bound, Complexity, Condition, Matcher, Metric, MetricCondition, TextMode, TraitRule,
};

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

/// The ids of the traits of a procedure, in the order of the rules. `module`
/// and `procedure` are indexes as in [`Evaluator::procedure_hits`], and
/// `metrics` are the metrics of that procedure.
///
/// The conditions that read the source read that procedure only, from its
/// declaration to its `End` line. An index out of range finds nothing in the
/// source, so only the conditions on `metrics` can hold.
pub fn procedure_traits(
    rules: &Complexity,
    evaluator: &Evaluator<'_>,
    module: usize,
    procedure: usize,
    metrics: &Metrics,
) -> Vec<String> {
    let subject = Subject {
        evaluator,
        module,
        procedure,
        metrics,
    };
    rules
        .traits
        .iter()
        .filter(|candidate| subject.has(&candidate.rule))
        .map(|candidate| candidate.id.clone())
        .collect()
}

/// A procedure, whose source the conditions read, and its metrics.
#[derive(Debug)]
struct Subject<'a, 'p> {
    evaluator: &'a Evaluator<'p>,
    module: usize,
    procedure: usize,
    metrics: &'a Metrics,
}

impl Subject<'_, '_> {
    /// Whether the rule of a trait holds. The loader refuses an empty list of
    /// conditions: `any` of none would never hold, `all` of none always would.
    fn has(&self, rule: &TraitRule) -> bool {
        match rule {
            TraitRule::Any(conditions) => conditions.iter().any(|condition| self.holds(condition)),
            TraitRule::All(conditions) => conditions.iter().all(|condition| self.holds(condition)),
            TraitRule::Structural(predicate) => self.finds(&Matcher::Structural(vec![*predicate])),
        }
    }

    /// Whether one condition of a rule holds.
    fn holds(&self, condition: &Condition) -> bool {
        match condition {
            Condition::Metric(bound) => within_bound(self.metrics, bound),
            Condition::Matches(matcher) => self.finds(matcher),
            Condition::Ratio {
                of,
                per_logical_loc_gte,
            } => self.reaches(of, *per_logical_loc_gte),
        }
    }

    /// Whether the occurrences of the members and chains of `of`, divided by
    /// the logical lines, reach `per_logical_loc_gte`. A name with a dot is a
    /// chain and any other is a member. Each use of a member counts, and each
    /// chain, once. Without a logical line there is no ratio: it never holds.
    fn reaches(&self, of: &[String], per_logical_loc_gte: f64) -> bool {
        let logical_loc = self.metrics.logical_loc;
        if logical_loc == 0 {
            return false;
        }
        let (chains, members): (Vec<String>, Vec<String>) =
            of.iter().cloned().partition(|name| name.contains('.'));
        let uses = self
            .count(&Matcher::Member(members))
            .saturating_add(self.count(&Matcher::Chain(chains)));
        let uses = u32::try_from(uses).unwrap_or(u32::MAX);
        // A division and not a product: as floats, 7 / 100 is 0.07 but
        // 0.07 * 100 is a little over 7, which would miss 7 uses in 100 lines.
        f64::from(uses) / f64::from(logical_loc) >= per_logical_loc_gte
    }

    /// Whether `matcher` finds anything in the procedure.
    fn finds(&self, matcher: &Matcher) -> bool {
        self.count(matcher) > 0
    }

    /// The number of hits of `matcher` in the procedure. Only the `regex`
    /// matcher reads the text mode, and a trait has none.
    fn count(&self, matcher: &Matcher) -> usize {
        let text = TextMode::Code;
        let hits = self
            .evaluator
            .procedure_hits(self.module, self.procedure, matcher, text);
        hits.len()
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
