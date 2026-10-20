// SPDX-License-Identifier: Apache-2.0
//! `security.yaml`: security signals, the chains that combine them, and the
//! container indicators raised by the extraction layer.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::matcher::{self, Matcher, MatcherKind, Patterns, Scope, TextMode};
use super::{Ids, Label, Problem, SECURITY_FILE, yaml};

/// The content of `security.yaml`.
#[derive(Debug, Clone)]
pub struct Security {
    pub version: String,
    pub signals: Vec<SecuritySignal>,
    pub chains: Vec<SecurityChain>,
    pub container_indicators: Vec<ContainerIndicator>,
}

/// How serious a security finding is, from `Info` to `High`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
}

/// A signal reported, with its severity, wherever it matches.
#[derive(Debug, Clone)]
pub struct SecuritySignal {
    pub id: String,
    /// The kind of behaviour, such as `execution` or `network`.
    pub category: String,
    pub matcher: Matcher,
    pub severity: Severity,
    pub scope: Scope,
    pub text: TextMode,
    /// Reported only in procedures whose primary or a secondary family is this one.
    pub requires_family: Option<String>,
    pub label: Label,
}

/// A finding raised when every term of `all` matches in the same file.
#[derive(Debug, Clone)]
pub struct SecurityChain {
    pub id: String,
    pub all: Vec<ChainTerm>,
    pub severity: Severity,
    pub label: Label,
}

/// A term of a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainTerm {
    /// Matches when the file has this signal.
    Signal(String),
    /// Matches when any of these terms does.
    Any(Vec<ChainTerm>),
}

/// A finding of the extraction layer (M1), such as a VBA project without
/// its source: the rules only give its severity and label.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerIndicator {
    pub id: String,
    pub severity: Severity,
    pub label: Label,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSecurity {
    version: String,
    signals: Vec<RawSignal>,
    chains: Vec<RawChain>,
    container_indicators: Vec<ContainerIndicator>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSignal {
    id: String,
    category: String,
    #[serde(rename = "match")]
    matcher: MatcherKind,
    pattern: Patterns,
    severity: Severity,
    #[serde(default)]
    scope: Scope,
    #[serde(default)]
    text: TextMode,
    requires_family: Option<String>,
    label: Label,
    /// Free text for rule authors, ignored.
    #[serde(rename = "note")]
    _note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChain {
    id: String,
    all: Vec<RawTerm>,
    severity: Severity,
    label: Label,
}

/// A chain term as written: a signal id or `{ any: [terms] }`.
#[derive(Deserialize)]
#[serde(
    untagged,
    expecting = "invalid chain term, expected a signal id or { any: [terms] }"
)]
enum RawTerm {
    Signal(String),
    Any(RawAny),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAny {
    any: Vec<RawTerm>,
}

/// Loads `security.yaml`. Problems come in the order of the signals, then
/// the chains, then the container indicators. `requires_family` is checked
/// against `family_ids`, the families of `families.yaml`, unless that file
/// does not parse.
pub(super) fn load(
    text: &str,
    family_ids: Option<&BTreeSet<&str>>,
) -> Result<Security, Vec<Problem>> {
    let raw: RawSecurity =
        yaml::from_str(text).map_err(|message| vec![Problem::in_file(SECURITY_FILE, message)])?;
    let mut problems = Vec::new();
    // Signals, chains and container indicators share one namespace.
    let mut ids = Ids::new(SECURITY_FILE, "id");
    // Every signal, its patterns valid or not, for the chain terms.
    let signal_ids: BTreeSet<String> = raw.signals.iter().map(|signal| signal.id.clone()).collect();
    let mut signals = Vec::with_capacity(raw.signals.len());
    for (index, signal) in raw.signals.into_iter().enumerate() {
        let at = format!("/signals/{index}");
        problems.extend(ids.record(&signal.id, &at));
        if let (Some(required), Some(family_ids)) = (&signal.requires_family, family_ids)
            && !family_ids.contains(required.as_str())
        {
            problems.push(Problem {
                file: SECURITY_FILE,
                rule: Some(signal.id.clone()),
                location: format!("{at}/requires_family"),
                message: format!("unknown family '{required}'"),
            });
        }
        match matcher::compile(signal.matcher, signal.pattern) {
            Ok(matcher) => signals.push(SecuritySignal {
                category: signal.category,
                matcher,
                severity: signal.severity,
                scope: signal.scope,
                text: signal.text,
                requires_family: signal.requires_family,
                label: signal.label,
                id: signal.id,
            }),
            Err(errors) => problems.extend(
                errors
                    .into_iter()
                    .map(|error| error.into_problem(SECURITY_FILE, &signal.id, &at)),
            ),
        }
    }
    let mut chains = Vec::with_capacity(raw.chains.len());
    for (index, chain) in raw.chains.into_iter().enumerate() {
        let at = format!("/chains/{index}");
        problems.extend(ids.record(&chain.id, &at));
        let all = checked_terms(
            chain.all,
            &format!("{at}/all"),
            &chain.id,
            &signal_ids,
            &mut problems,
        );
        chains.push(SecurityChain {
            id: chain.id,
            all,
            severity: chain.severity,
            label: chain.label,
        });
    }
    for (index, indicator) in raw.container_indicators.iter().enumerate() {
        problems.extend(ids.record(&indicator.id, &format!("/container_indicators/{index}")));
    }
    if problems.is_empty() {
        Ok(Security {
            version: raw.version,
            signals,
            chains,
            container_indicators: raw.container_indicators,
        })
    } else {
        Err(problems)
    }
}

/// The terms of the list at `at` in the chain `chain_id`, each signal id
/// checked against `signal_ids`. The YAML depth budget bounds the recursion.
fn checked_terms(
    raw: Vec<RawTerm>,
    at: &str,
    chain_id: &str,
    signal_ids: &BTreeSet<String>,
    problems: &mut Vec<Problem>,
) -> Vec<ChainTerm> {
    // An empty `all` would match every file, an empty `any` none.
    if raw.is_empty() {
        problems.push(Problem {
            file: SECURITY_FILE,
            rule: Some(chain_id.to_owned()),
            location: at.to_owned(),
            message: "empty list of terms".to_owned(),
        });
    }
    let mut terms = Vec::with_capacity(raw.len());
    for (index, term) in raw.into_iter().enumerate() {
        let term_at = format!("{at}/{index}");
        terms.push(match term {
            RawTerm::Signal(id) => {
                if !signal_ids.contains(&id) {
                    problems.push(Problem {
                        file: SECURITY_FILE,
                        rule: Some(chain_id.to_owned()),
                        location: term_at,
                        message: format!("unknown signal '{id}'"),
                    });
                }
                ChainTerm::Signal(id)
            }
            RawTerm::Any(any) => ChainTerm::Any(checked_terms(
                any.any,
                &format!("{term_at}/any"),
                chain_id,
                signal_ids,
                problems,
            )),
        });
    }
    terms
}
