// SPDX-License-Identifier: Apache-2.0
//! Rules: the YAML files of `rules/`, embedded at build time or supplied by
//! the caller (`--rules`), loaded into checked, typed rules.
//!
//! Loading reports every problem it finds, each with its file, the closest
//! rule id and a JSON Pointer to the value at fault. Validation against the
//! JSON Schemas of `schema/rules/` belongs to the CLI; [`parse_value`] gives it
//! the document to validate.

mod complexity;
mod families;
mod matcher;
mod security;
mod yaml;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;

use serde::Deserialize;

pub use complexity::{Bound, Complexity, Condition, FileLevels, MetricCondition, Trait, TraitRule};
pub use families::{Families, Family, FamilySignal, Scoring};
pub use matcher::{Matcher, ModuleType, Scope, StructuralPredicate, TextMode};
pub use security::{
    ChainTerm, ContainerIndicator, Security, SecurityChain, SecuritySignal, Severity,
};
pub use yaml::parse_value;

/// File name of the usage families rules.
pub const FAMILIES_FILE: &str = "families.yaml";

/// File name of the security rules.
pub const SECURITY_FILE: &str = "security.yaml";

/// File name of the complexity rules.
pub const COMPLEXITY_FILE: &str = "complexity.yaml";

/// The text of each rules file.
#[derive(Debug, Clone, Copy)]
pub struct RulesSources<'a> {
    /// Content of `families.yaml`.
    pub families: &'a str,
    /// Content of `security.yaml`.
    pub security: &'a str,
    /// Content of `complexity.yaml`.
    pub complexity: &'a str,
}

impl RulesSources<'static> {
    /// The rules shipped in `rules/`, embedded at build time.
    pub const EMBEDDED: Self = Self {
        families: include_str!("../../../../rules/families.yaml"),
        security: include_str!("../../../../rules/security.yaml"),
        complexity: include_str!("../../../../rules/complexity.yaml"),
    };
}

/// Checked, typed rules.
#[derive(Debug, Clone)]
pub struct RuleSet {
    /// Usage families and their signals.
    pub families: Families,
    /// Security signals, chains and container indicators.
    pub security: Security,
    /// File complexity levels and traits.
    pub complexity: Complexity,
}

impl RuleSet {
    /// Loads every rules file.
    ///
    /// # Errors
    ///
    /// Every problem found in every file, in the order `families.yaml`,
    /// `security.yaml`, `complexity.yaml`. The families that
    /// `requires_family` names are checked only when `families.yaml` parses.
    pub fn load(sources: &RulesSources<'_>) -> Result<Self, Vec<Problem>> {
        let parsed = families::parse(sources.families);
        let family_ids = parsed.as_ref().ok().map(families::RawFamilies::family_ids);
        let security = security::load(sources.security, family_ids.as_ref());
        let families = parsed.and_then(families::check);
        let complexity = complexity::load(sources.complexity);
        match (families, security, complexity) {
            (Ok(families), Ok(security), Ok(complexity)) => Ok(Self {
                families,
                security,
                complexity,
            }),
            (families, security, complexity) => Err(families
                .err()
                .into_iter()
                .chain(security.err())
                .chain(complexity.err())
                .flatten()
                .collect()),
        }
    }
}

/// A user-facing label, in English and French.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub en: String,
    pub fr: String,
}

/// One problem in a rules file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file at fault, such as `families.yaml`.
    pub file: &'static str,
    /// Id of the closest rule holding the problem: a family, a signal...
    pub rule: Option<String>,
    /// JSON Pointer to the value at fault, empty for the whole file.
    pub location: String,
    /// What is wrong.
    pub message: String,
}

impl Problem {
    /// A problem with the whole file, such as a YAML syntax error.
    pub fn in_file(file: &'static str, message: String) -> Self {
        Self {
            file,
            rule: None,
            location: String::new(),
            message,
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.file)?;
        if let Some(rule) = &self.rule {
            write!(f, "{rule}: ")?;
        }
        f.write_str(&self.message)?;
        if !self.location.is_empty() {
            write!(f, " (at {})", self.location)?;
        }
        Ok(())
    }
}

/// The ids that share a namespace in a rules file, to report duplicates.
struct Ids {
    file: &'static str,
    /// What the ids are called in messages, such as `family id`.
    what: &'static str,
    /// Where each id is first defined.
    first: BTreeMap<String, String>,
}

impl Ids {
    fn new(file: &'static str, what: &'static str) -> Self {
        Self {
            file,
            what,
            first: BTreeMap::new(),
        }
    }

    /// Records that `id` is defined at `at`, or reports where it was first defined.
    fn record(&mut self, id: &str, at: &str) -> Option<Problem> {
        match self.first.entry(id.to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(at.to_owned());
                None
            }
            Entry::Occupied(entry) => Some(Problem {
                file: self.file,
                rule: Some(id.to_owned()),
                location: format!("{at}/id"),
                message: format!("duplicate {}, first defined at {}", self.what, entry.get()),
            }),
        }
    }
}
