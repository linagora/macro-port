// SPDX-License-Identifier: Apache-2.0
//! Rules: the YAML files of `rules/`, embedded at build time or supplied by
//! the caller (`--rules`), loaded into checked, typed rules.
//!
//! Loading reports every problem it finds, each with its file, the closest
//! rule id and a JSON Pointer to the value at fault. Validation against the
//! JSON Schemas of `schema/rules/` belongs to the CLI; [`parse_value`] gives it
//! the document to validate.

mod families;
mod matcher;
mod yaml;

use std::fmt;

use serde::Deserialize;

pub use families::{Families, Family, FamilySignal, Scoring};
pub use matcher::{Matcher, ModuleType, Scope, StructuralPredicate, TextMode};
pub use yaml::parse_value;

/// File name of the usage families rules.
pub const FAMILIES_FILE: &str = "families.yaml";

/// The text of each rules file.
#[derive(Debug, Clone, Copy)]
pub struct RulesSources<'a> {
    /// Content of `families.yaml`.
    pub families: &'a str,
}

impl RulesSources<'static> {
    /// The rules shipped in `rules/`, embedded at build time.
    pub const EMBEDDED: Self = Self {
        families: include_str!("../../../../rules/families.yaml"),
    };
}

/// Checked, typed rules.
#[derive(Debug, Clone)]
pub struct RuleSet {
    /// Usage families and their signals.
    pub families: Families,
}

impl RuleSet {
    /// Loads every rules file.
    ///
    /// # Errors
    ///
    /// Every problem found, in document order.
    pub fn load(sources: &RulesSources<'_>) -> Result<Self, Vec<Problem>> {
        let families = families::load(sources.families)?;
        Ok(Self { families })
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
