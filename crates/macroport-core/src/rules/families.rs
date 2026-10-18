// SPDX-License-Identifier: Apache-2.0
//! `families.yaml`: usage families and their weighted signals.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use serde::Deserialize;

use super::matcher::{self, Matcher, MatcherKind, Patterns, Scope, TextMode};
use super::{FAMILIES_FILE, Label, Problem, yaml};

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFamilies {
    version: String,
    scoring: Scoring,
    families: Vec<RawFamily>,
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
    weight: u32,
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

/// Loads `families.yaml`.
pub(super) fn load(text: &str) -> Result<Families, Vec<Problem>> {
    let raw: RawFamilies =
        yaml::from_str(text).map_err(|message| vec![Problem::in_file(FAMILIES_FILE, message)])?;
    let default_max_hits = raw.scoring.default_max_hits_per_procedure;
    let mut problems = Vec::new();
    let mut family_ids = BTreeMap::new();
    let mut signal_ids = BTreeMap::new();
    let mut families = Vec::with_capacity(raw.families.len());
    for (family_index, family) in raw.families.into_iter().enumerate() {
        let family_at = format!("/families/{family_index}");
        problems.extend(record_id(&mut family_ids, "family", &family.id, &family_at));
        let mut signals = Vec::with_capacity(family.signals.len());
        for (signal_index, signal) in family.signals.into_iter().enumerate() {
            let signal_at = format!("{family_at}/signals/{signal_index}");
            problems.extend(record_id(&mut signal_ids, "signal", &signal.id, &signal_at));
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
                Err(errors) => problems.extend(errors.into_iter().map(|error| Problem {
                    file: FAMILIES_FILE,
                    rule: Some(signal.id.clone()),
                    location: format!("{signal_at}/pattern{}", error.suffix),
                    message: error.message,
                })),
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

/// Records that `id` is defined at `at`, or reports where it was first defined.
fn record_id(
    seen: &mut BTreeMap<String, String>,
    what: &str,
    id: &str,
    at: &str,
) -> Option<Problem> {
    match seen.entry(id.to_owned()) {
        Entry::Vacant(entry) => {
            entry.insert(at.to_owned());
            None
        }
        Entry::Occupied(entry) => Some(Problem {
            file: FAMILIES_FILE,
            rule: Some(id.to_owned()),
            location: format!("{at}/id"),
            message: format!("duplicate {what} id, first defined at {}", entry.get()),
        }),
    }
}
