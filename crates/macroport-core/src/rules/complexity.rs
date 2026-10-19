// SPDX-License-Identifier: Apache-2.0
//! `complexity.yaml`: the metric thresholds of the file complexity levels.
//! Its traits are not read yet.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;

use super::{COMPLEXITY_FILE, Problem, yaml};

/// The content of `complexity.yaml`.
#[derive(Debug, Clone)]
pub struct Complexity {
    pub version: String,
    pub file_levels: FileLevels,
    /// The metrics that conditions may name, each with its documentation.
    pub metric_definitions: BTreeMap<String, String>,
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
    pub metric: String,
    pub bound: Bound,
}

/// The `gte` or `lte` of a metric condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The metric is at least this value.
    AtLeast(u32),
    /// The metric is at most this value.
    AtMost(u32),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawComplexity {
    version: String,
    file_levels: RawFileLevels,
    metric_definitions: BTreeMap<String, String>,
    /// Not read yet.
    #[serde(rename = "traits")]
    _traits: IgnoredAny,
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

/// Loads `complexity.yaml`. Problems come in the order of the file levels.
pub(super) fn load(text: &str) -> Result<Complexity, Vec<Problem>> {
    let RawComplexity {
        version,
        file_levels,
        metric_definitions,
        ..
    } = yaml::from_str(text).map_err(|message| vec![Problem::in_file(COMPLEXITY_FILE, message)])?;
    let mut checker = Checker {
        metrics: &metric_definitions,
        problems: Vec::new(),
    };
    let file_levels = FileLevels {
        medium: checker.level(file_levels.medium, "/file_levels/medium"),
        high: checker.level(file_levels.high, "/file_levels/high"),
    };
    let Checker { problems, .. } = checker;
    if problems.is_empty() {
        Ok(Complexity {
            version,
            file_levels,
            metric_definitions,
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
        self.not_empty(&level.any, &at);
        level
            .any
            .into_iter()
            .enumerate()
            .filter_map(|(index, raw)| self.metric(raw, &format!("{at}/{index}")))
            .collect()
    }

    /// The metric condition at `at`.
    fn metric(&mut self, raw: RawMetricCondition, at: &str) -> Option<MetricCondition> {
        let bound = match (raw.gte, raw.lte) {
            (Some(value), None) => Some(Bound::AtLeast(value)),
            (None, Some(value)) => Some(Bound::AtMost(value)),
            _ => {
                let message = "needs exactly one of: gte, lte".to_owned();
                self.push(at.to_owned(), message);
                None
            }
        };
        if !self.metrics.contains_key(&raw.metric) {
            let message = format!(
                "metric '{}' is not defined in metric_definitions",
                raw.metric
            );
            self.push(format!("{at}/metric"), message);
            return None;
        }
        bound.map(|bound| MetricCondition {
            metric: raw.metric,
            bound,
        })
    }

    /// Reports the list at `at` if it is empty: an empty `any` would never hold.
    fn not_empty<T>(&mut self, list: &[T], at: &str) {
        if list.is_empty() {
            let message = "empty list of conditions".to_owned();
            self.push(at.to_owned(), message);
        }
    }

    fn push(&mut self, location: String, message: String) {
        self.problems.push(Problem {
            file: COMPLEXITY_FILE,
            rule: None,
            location,
            message,
        });
    }
}
