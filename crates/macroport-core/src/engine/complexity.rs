// SPDX-License-Identifier: Apache-2.0
//! The complexity level of a file and the traits of its procedures and of the
//! file itself (SPEC M3), from `rules/complexity.yaml`.
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
    let scope = Scope::Procedure { module, procedure };
    traits(
        rules,
        &Subject {
            evaluator,
            scope,
            metrics,
        },
    )
}

/// The ids of the traits of a file, in the order of the rules. `metrics` are
/// the metrics of the file.
///
/// A rule applies to the file as it does to a procedure: it bounds the metrics
/// of the file, and its conditions on the source read every module, so that a
/// comment, a procedure name or a structural predicate is found anywhere in
/// the project and a ratio counts the uses in the project against the logical
/// lines of the file. The traits of the procedures are not gathered: a file
/// with many procedures is not `trivial` because one of them is.
///
/// A file with no code has the traits that its metrics give, `trivial` among
/// them. Whether such a file has traits is for the caller to decide.
pub fn file_traits(
    rules: &Complexity,
    evaluator: &Evaluator<'_>,
    metrics: &Metrics,
) -> Vec<String> {
    traits(
        rules,
        &Subject {
            evaluator,
            scope: Scope::File,
            metrics,
        },
    )
}

/// What the traits are about: where the source is read, and the metrics.
#[derive(Debug)]
struct Subject<'a, 'p> {
    evaluator: &'a Evaluator<'p>,
    scope: Scope,
    metrics: &'a Metrics,
}

/// The part of the project that the conditions on the source read.
#[derive(Debug, Clone, Copy)]
enum Scope {
    Procedure { module: usize, procedure: usize },
    File,
}

/// The ids of the traits that hold of `subject`, in the order of the rules.
fn traits(rules: &Complexity, subject: &Subject<'_, '_>) -> Vec<String> {
    rules
        .traits
        .iter()
        .filter(|candidate| subject.has(&candidate.rule))
        .map(|candidate| candidate.id.clone())
        .collect()
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

    /// Whether `matcher` finds anything in the part of the project that the
    /// traits are about.
    fn finds(&self, matcher: &Matcher) -> bool {
        self.count(matcher) > 0
    }

    /// The number of hits of `matcher` in the part of the project that the
    /// traits are about. Only the `regex` matcher reads the text mode, and a
    /// trait has none.
    fn count(&self, matcher: &Matcher) -> usize {
        let text = TextMode::Code;
        let hits = match self.scope {
            Scope::Procedure { module, procedure } => self
                .evaluator
                .procedure_hits(module, procedure, matcher, text),
            Scope::File => self.evaluator.project_hits(matcher, text),
        };
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
