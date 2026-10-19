// SPDX-License-Identifier: Apache-2.0
//! `families.yaml`: usage families and their weighted signals.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::matcher::{self, Matcher, MatcherKind, Patterns, Scope, TextMode};
use super::{FAMILIES_FILE, Ids, Label, Problem, yaml};

/// The content of `families.yaml`.
#[derive(Debug, Clone)]
pub struct Families {
    pub version: String,
    pub scoring: Scoring,
    pub families: Vec<Family>,
}

/// How signal weights add up to family scores (SPEC M3).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scoring {
    /// Below this score, a procedure is unclassified.
    pub min_score: f64,
    /// Secondary families score at least this ratio of the primary score.
    pub secondary_ratio: f64,
    /// Cap on the hits counted per signal and procedure, unless the signal sets its own.
    #[serde(deserialize_with = "yaml::whole_number")]
    pub default_max_hits_per_procedure: u32,
}

/// A usage family.
#[derive(Debug, Clone)]
pub struct Family {
    pub id: String,
    /// The corpus folder and manifest `family` of this family.
    pub corpus_slug: String,
    pub label: Label,
    pub signals: Vec<FamilySignal>,
}

/// A signal that adds `weight` to its family's score.
#[derive(Debug, Clone)]
pub struct FamilySignal {
    pub id: String,
    pub matcher: Matcher,
    pub weight: u32,
    /// The signal's own cap, or the scoring default.
    pub max_hits_per_procedure: u32,
    pub scope: Scope,
    pub text: TextMode,
    pub label: Option<Label>,
}

/// `families.yaml` as parsed, before [`check`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawFamilies {
    version: String,
    scoring: Scoring,
    families: Vec<RawFamily>,
}

impl RawFamilies {
    /// The id of every family, whether [`check`] accepts it or not.
    pub(super) fn family_ids(&self) -> BTreeSet<&str> {
        self.families
            .iter()
            .map(|family| family.id.as_str())
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFamily {
    id: String,
    corpus_slug: String,
    label: Label,
    signals: Vec<RawSignal>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSignal {
    id: String,
    #[serde(rename = "match")]
    matcher: MatcherKind,
    pattern: Patterns,
    #[serde(deserialize_with = "yaml::whole_number")]
    weight: u32,
    #[serde(default, deserialize_with = "yaml::optional_whole_number")]
    max_hits_per_procedure: Option<u32>,
    #[serde(default)]
    scope: Scope,
    #[serde(default)]
    text: TextMode,
    label: Option<Label>,
    /// Free text for rule authors, ignored.
    #[serde(rename = "note")]
    _note: Option<String>,
}

/// Parses `families.yaml`.
pub(super) fn parse(text: &str) -> Result<RawFamilies, Vec<Problem>> {
    yaml::from_str(text).map_err(|message| vec![Problem::in_file(FAMILIES_FILE, message)])
}

/// Checks parsed `families.yaml` into typed families.
pub(super) fn check(raw: RawFamilies) -> Result<Families, Vec<Problem>> {
    let default_max_hits = raw.scoring.default_max_hits_per_procedure;
    let mut problems = Vec::new();
    let mut family_ids = Ids::new(FAMILIES_FILE, "family id");
    let mut signal_ids = Ids::new(FAMILIES_FILE, "signal id");
    let mut families = Vec::with_capacity(raw.families.len());
    for (family_index, family) in raw.families.into_iter().enumerate() {
        let family_at = format!("/families/{family_index}");
        problems.extend(family_ids.record(&family.id, &family_at));
        let mut signals = Vec::with_capacity(family.signals.len());
        for (signal_index, signal) in family.signals.into_iter().enumerate() {
            let signal_at = format!("{family_at}/signals/{signal_index}");
            problems.extend(signal_ids.record(&signal.id, &signal_at));
            match matcher::compile(signal.matcher, signal.pattern) {
                Ok(matcher) => signals.push(FamilySignal {
                    matcher,
                    weight: signal.weight,
                    max_hits_per_procedure: signal
                        .max_hits_per_procedure
                        .unwrap_or(default_max_hits),
                    scope: signal.scope,
                    text: signal.text,
                    label: signal.label,
                    id: signal.id,
                }),
                Err(errors) => problems.extend(
                    errors
                        .into_iter()
                        .map(|error| error.into_problem(FAMILIES_FILE, &signal.id, &signal_at)),
                ),
            }
        }
        families.push(Family {
            id: family.id,
            corpus_slug: family.corpus_slug,
            label: family.label,
            signals,
        });
    }
    if problems.is_empty() {
        Ok(Families {
            version: raw.version,
            scoring: raw.scoring,
            families,
        })
    } else {
        Err(problems)
    }
}
