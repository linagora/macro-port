// SPDX-License-Identifier: Apache-2.0
//! `complexity.yaml`: the metric thresholds of the file complexity levels,
//! and the traits of procedures and files.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::matcher::{self, Matcher, Patterns, StructuralPredicate};
use super::{COMPLEXITY_FILE, Ids, Label, Problem, yaml};

/// The content of `complexity.yaml`.
#[derive(Debug, Clone)]
pub struct Complexity {
    pub version: String,
    pub file_levels: FileLevels,
    /// The metrics that conditions may name, each with its documentation.
    pub metric_definitions: BTreeMap<String, String>,
    pub traits: Vec<Trait>,
}

/// The conditions that raise a file to each level: any one is enough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLevels {
    pub medium: Vec<MetricCondition>,
    pub high: Vec<MetricCondition>,
}

/// A bound on a metric of `metric_definitions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricCondition {
    pub metric: Metric,
    pub bound: Bound,
}

/// A metric that conditions can bound: one of the metrics of a report, under
/// the name that `report::Metrics` gives it. `metric_definitions` documents
/// them, and a rules file defines only these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    Loc,
    LogicalLoc,
    Procedures,
    /// Of a procedure, its own; of a file, the highest of its procedures.
    Cyclomatic,
    CyclomaticTotal,
    MaxNesting,
    Parameters,
    GotoCount,
    Forms,
    DeclareCount,
    ExternalDependencies,
}

impl Metric {
    /// Every metric, in the order of the report.
    pub const ALL: [Self; 11] = [
        Self::Loc,
        Self::LogicalLoc,
        Self::Procedures,
        Self::Cyclomatic,
        Self::CyclomaticTotal,
        Self::MaxNesting,
        Self::Parameters,
        Self::GotoCount,
        Self::Forms,
        Self::DeclareCount,
        Self::ExternalDependencies,
    ];

    /// The name used in rules and reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Loc => "loc",
            Self::LogicalLoc => "logical_loc",
            Self::Procedures => "procedures",
            Self::Cyclomatic => "cyclomatic",
            Self::CyclomaticTotal => "cyclomatic_total",
            Self::MaxNesting => "max_nesting",
            Self::Parameters => "parameters",
            Self::GotoCount => "goto_count",
            Self::Forms => "forms",
            Self::DeclareCount => "declare_count",
            Self::ExternalDependencies => "external_dependencies",
        }
    }

    /// Finds a metric by its name, or says which names exist.
    fn from_name(name: &str) -> Result<Self, String> {
        matcher::closed("metric", name, Self::ALL, Self::name)
    }
}

/// The `gte` or `lte` of a metric condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The metric is at least this value.
    AtLeast(u32),
    /// The metric is at most this value.
    AtMost(u32),
}

/// A trait of procedures and files, such as `recorded_macro`.
#[derive(Debug, Clone)]
pub struct Trait {
    pub id: String,
    pub label: Label,
    pub description: String,
    pub rule: TraitRule,
}

/// What gives a trait.
#[derive(Debug, Clone)]
pub enum TraitRule {
    /// Any one of the conditions holds.
    Any(Vec<Condition>),
    /// Every condition holds.
    All(Vec<Condition>),
    /// The structural predicate holds.
    Structural(StructuralPredicate),
}

/// A condition of a trait. Every regex is case-insensitive unless an inline
/// flag says otherwise.
#[derive(Debug, Clone)]
pub enum Condition {
    /// A bound on a metric.
    Metric(MetricCondition),
    /// The matcher finds something: `Matcher::Comment` for `match: comment`
    /// (regexes on comment text) and `Matcher::ProcedureName` for
    /// `match: procedure_name` (regexes on the procedure name).
    Matches(Matcher),
    /// The occurrences of these members or chains, divided by the
    /// `logical_loc`, reach `per_logical_loc_gte`. A name of `of` that holds a
    /// dot is a chain and any other is a member. For a procedure, the
    /// occurrences and the `logical_loc` are its own; for a file, the
    /// occurrences are those of every module and the `logical_loc` is the
    /// file's. The schema requires `of` not to be empty and the threshold to be
    /// positive, and the loader leaves that to it.
    Ratio {
        of: Vec<String>,
        per_logical_loc_gte: f64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComplexity {
    version: String,
    file_levels: RawFileLevels,
    metric_definitions: BTreeMap<String, String>,
    traits: Vec<RawTrait>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFileLevels {
    medium: RawLevel,
    high: RawLevel,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLevel {
    any: Vec<RawMetricCondition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMetricCondition {
    metric: String,
    #[serde(default, deserialize_with = "yaml::optional_whole_number")]
    gte: Option<u32>,
    #[serde(default, deserialize_with = "yaml::optional_whole_number")]
    lte: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrait {
    id: String,
    label: Label,
    description: String,
    any: Option<Vec<RawCondition>>,
    all: Option<Vec<RawCondition>>,
    structural: Option<String>,
}

/// A trait condition as written, told apart by its keys.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "invalid condition, expected { metric, gte | lte }, \
                 { match: comment | procedure_name, pattern } \
                 or { match: ratio, of, per_logical_loc_gte }"
)]
enum RawCondition {
    Metric(RawMetricCondition),
    Pattern(RawPatternCondition),
    Ratio(RawRatioCondition),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPatternCondition {
    #[serde(rename = "match")]
    matcher: PatternMatch,
    pattern: Patterns,
}

/// The `match` key of a condition on patterns.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PatternMatch {
    Comment,
    ProcedureName,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRatioCondition {
    /// Always `ratio`.
    #[serde(rename = "match")]
    _matcher: RatioMatch,
    of: Vec<String>,
    per_logical_loc_gte: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RatioMatch {
    Ratio,
}

/// Loads `complexity.yaml`. Problems come in the order of the file levels,
/// the metric definitions, then the traits.
pub(super) fn load(text: &str) -> Result<Complexity, Vec<Problem>> {
    let RawComplexity {
        version,
        file_levels,
        metric_definitions,
        traits: raw_traits,
    } = yaml::from_str(text).map_err(|message| vec![Problem::in_file(COMPLEXITY_FILE, message)])?;
    let mut checker = Checker {
        metrics: &metric_definitions,
        problems: Vec::new(),
    };
    let file_levels = FileLevels {
        medium: checker.level(file_levels.medium, "/file_levels/medium"),
        high: checker.level(file_levels.high, "/file_levels/high"),
    };
    checker.check_definitions();
    let mut ids = Ids::new(COMPLEXITY_FILE, "trait id");
    let mut traits = Vec::with_capacity(raw_traits.len());
    for (index, raw) in raw_traits.into_iter().enumerate() {
        let at = format!("/traits/{index}");
        checker.problems.extend(ids.record(&raw.id, &at));
        traits.extend(checker.checked_trait(raw, &at));
    }
    let Checker { problems, .. } = checker;
    if problems.is_empty() {
        Ok(Complexity {
            version,
            file_levels,
            metric_definitions,
            traits,
        })
    } else {
        Err(problems)
    }
}

/// Checks the conditions of `complexity.yaml` and collects their problems.
/// What it returns is complete only while it has found no problem.
struct Checker<'a> {
    /// The metrics of `metric_definitions`.
    metrics: &'a BTreeMap<String, String>,
    problems: Vec<Problem>,
}

impl Checker<'_> {
    /// The conditions of a file level, at `at`.
    fn level(&mut self, level: RawLevel, at: &str) -> Vec<MetricCondition> {
        let at = format!("{at}/any");
        self.not_empty(None, &level.any, &at);
        level
            .any
            .into_iter()
            .enumerate()
            .filter_map(|(index, raw)| self.metric(None, raw, &format!("{at}/{index}")))
            .collect()
    }

    /// The trait at `at`. Every rule it has is checked, so that one run
    /// reports every problem.
    fn checked_trait(&mut self, raw: RawTrait, at: &str) -> Option<Trait> {
        let RawTrait {
            id,
            label,
            description,
            any,
            all,
            structural,
        } = raw;
        let given = [any.is_some(), all.is_some(), structural.is_some()];
        if given.into_iter().filter(|is_given| *is_given).count() != 1 {
            let message = "needs exactly one of: any, all, structural".to_owned();
            self.push(Some(&id), at.to_owned(), message);
        }
        let any = any.map(|list| TraitRule::Any(self.conditions(&id, list, &format!("{at}/any"))));
        let all = all.map(|list| TraitRule::All(self.conditions(&id, list, &format!("{at}/all"))));
        let structural = structural.and_then(|name| match matcher::structural_predicate(&name) {
            Ok(predicate) => Some(TraitRule::Structural(predicate)),
            Err(message) => {
                self.push(Some(&id), format!("{at}/structural"), message);
                None
            }
        });
        let rule = any.or(all).or(structural)?;
        Some(Trait {
            id,
            label,
            description,
            rule,
        })
    }

    /// The conditions of the list at `at` in the rule `rule`.
    fn conditions(&mut self, rule: &str, list: Vec<RawCondition>, at: &str) -> Vec<Condition> {
        self.not_empty(Some(rule), &list, at);
        let mut conditions = Vec::with_capacity(list.len());
        for (index, raw) in list.into_iter().enumerate() {
            let at = format!("{at}/{index}");
            match raw {
                RawCondition::Metric(raw) => {
                    conditions.extend(self.metric(Some(rule), raw, &at).map(Condition::Metric));
                }
                RawCondition::Pattern(raw) => match matcher::regexes(raw.pattern) {
                    Ok(regexes) => conditions.push(Condition::Matches(match raw.matcher {
                        PatternMatch::Comment => Matcher::Comment(regexes),
                        PatternMatch::ProcedureName => Matcher::ProcedureName(regexes),
                    })),
                    Err(errors) => self.problems.extend(
                        errors
                            .into_iter()
                            .map(|error| error.into_problem(COMPLEXITY_FILE, rule, &at)),
                    ),
                },
                RawCondition::Ratio(raw) => conditions.push(Condition::Ratio {
                    of: raw.of,
                    per_logical_loc_gte: raw.per_logical_loc_gte,
                }),
            }
        }
        conditions
    }

    /// The metric condition at `at`, in the rule `rule` if any.
    fn metric(
        &mut self,
        rule: Option<&str>,
        raw: RawMetricCondition,
        at: &str,
    ) -> Option<MetricCondition> {
        let bound = match (raw.gte, raw.lte) {
            (Some(value), None) => Some(Bound::AtLeast(value)),
            (None, Some(value)) => Some(Bound::AtMost(value)),
            _ => {
                let message = "needs exactly one of: gte, lte".to_owned();
                self.push(rule, at.to_owned(), message);
                None
            }
        };
        let metric = match Metric::from_name(&raw.metric) {
            Ok(metric) => metric,
            Err(message) => {
                self.push(rule, format!("{at}/metric"), message);
                return None;
            }
        };
        if !self.metrics.contains_key(metric.name()) {
            let message = format!(
                "metric '{}' is not defined in metric_definitions",
                metric.name()
            );
            self.push(rule, format!("{at}/metric"), message);
            return None;
        }
        bound.map(|bound| MetricCondition { metric, bound })
    }

    /// Reports the keys of `metric_definitions` that name no metric of the
    /// report: no condition could use what they define.
    fn check_definitions(&mut self) {
        let metrics = self.metrics;
        for name in metrics.keys() {
            if let Err(message) = Metric::from_name(name) {
                let at = format!("/metric_definitions/{}", pointer_segment(name));
                self.push(None, at, message);
            }
        }
    }

    /// Reports the list at `at` if it is empty: an empty `any` would never
    /// hold, an empty `all` always would.
    fn not_empty<T>(&mut self, rule: Option<&str>, list: &[T], at: &str) {
        if list.is_empty() {
            let message = "empty list of conditions".to_owned();
            self.push(rule, at.to_owned(), message);
        }
    }

    fn push(&mut self, rule: Option<&str>, location: String, message: String) {
        self.problems.push(Problem {
            file: COMPLEXITY_FILE,
            rule: rule.map(str::to_owned),
            location,
            message,
        });
    }
}

/// A mapping key as a segment of a JSON Pointer, where `~` and `/` are escaped.
fn pointer_segment(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
