// SPDX-License-Identifier: Apache-2.0
//! Signal matchers and structural predicates (SPEC section 7).

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use super::Problem;

/// The `match` key of a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MatcherKind {
    Member,
    Chain,
    Call,
    Progid,
    ProcedureName,
    ModuleType,
    Reference,
    Regex,
    String,
    Comment,
    Structural,
}

/// The `pattern` key of a signal: one value or a list.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Patterns {
    One(String),
    Many(Vec<String>),
}

/// What a signal matches. Names compare case-insensitively and every regex
/// is case-insensitive unless an inline flag says otherwise.
#[derive(Debug, Clone)]
pub enum Matcher {
    /// Identifiers used as members (`.X`, `!X`) or standalone object names.
    Member(Vec<String>),
    /// Dot-separated member chains, such as `Workbooks.Open`, that the rule
    /// engine finds anywhere in a chain, not only at its end.
    Chain(Vec<String>),
    /// Procedures, functions or statements invoked by name.
    Call(Vec<String>),
    /// ProgID globs passed to `CreateObject` or `GetObject`.
    ProgId(Vec<String>),
    /// Regexes on procedure names.
    ProcedureName(Vec<Regex>),
    /// Module types.
    ModuleType(Vec<ModuleType>),
    /// Regexes on the display names of the VBA project references.
    Reference(Vec<Regex>),
    /// Regexes on logical code lines.
    Regex(Vec<Regex>),
    /// Regexes on string literal contents.
    String(Vec<Regex>),
    /// Regexes on comment text.
    Comment(Vec<Regex>),
    /// Structural predicates implemented in Rust.
    Structural(Vec<StructuralPredicate>),
}

/// The type of a VBA module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleType {
    Standard,
    Class,
    Document,
    Form,
}

impl ModuleType {
    /// Every module type.
    pub const ALL: [Self; 4] = [Self::Standard, Self::Class, Self::Document, Self::Form];

    /// The name used in rules and reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Class => "class",
            Self::Document => "document",
            Self::Form => "form",
        }
    }
}

/// The closed list of structural predicates of Lot 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StructuralPredicate {
    UdfCandidate,
    EventHandler,
    AutoExec,
    ChrObfuscation,
    LongHexOrBase64Literal,
    NestedLoopCompare,
    PossiblyUnused,
    ParseRecovered,
}

impl StructuralPredicate {
    /// Every structural predicate.
    pub const ALL: [Self; 8] = [
        Self::UdfCandidate,
        Self::EventHandler,
        Self::AutoExec,
        Self::ChrObfuscation,
        Self::LongHexOrBase64Literal,
        Self::NestedLoopCompare,
        Self::PossiblyUnused,
        Self::ParseRecovered,
    ];

    /// The name used in rules.
    pub fn name(self) -> &'static str {
        match self {
            Self::UdfCandidate => "udf_candidate",
            Self::EventHandler => "event_handler",
            Self::AutoExec => "auto_exec",
            Self::ChrObfuscation => "chr_obfuscation",
            Self::LongHexOrBase64Literal => "long_hex_or_base64_literal",
            Self::NestedLoopCompare => "nested_loop_compare",
            Self::PossiblyUnused => "possibly_unused",
            Self::ParseRecovered => "parse_recovered",
        }
    }
}

/// Where a signal applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Procedure,
    Module,
    Project,
}

/// What a `regex` signal reads: code with strings and comments blanked, or
/// the raw line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMode {
    #[default]
    Code,
    Raw,
}

/// A pattern that is not valid for its matcher.
pub(super) struct PatternError {
    /// Location under `pattern`: empty for a single pattern, `/index` in a list.
    pub suffix: String,
    pub message: String,
}

impl PatternError {
    /// The problem of the rule `rule` of `file`, whose `pattern` key is in
    /// the mapping at `at`.
    pub(super) fn into_problem(self, file: &'static str, rule: &str, at: &str) -> Problem {
        Problem {
            file,
            rule: Some(rule.to_owned()),
            location: format!("{at}/pattern{}", self.suffix),
            message: self.message,
        }
    }
}

/// Builds the matcher of a signal, or returns every invalid pattern.
pub(super) fn compile(kind: MatcherKind, patterns: Patterns) -> Result<Matcher, Vec<PatternError>> {
    let patterns = numbered(patterns);
    match kind {
        MatcherKind::Member => Ok(Matcher::Member(texts(patterns))),
        MatcherKind::Chain => Ok(Matcher::Chain(texts(patterns))),
        MatcherKind::Call => Ok(Matcher::Call(texts(patterns))),
        MatcherKind::Progid => Ok(Matcher::ProgId(texts(patterns))),
        MatcherKind::ProcedureName => each(patterns, regex).map(Matcher::ProcedureName),
        MatcherKind::ModuleType => each(patterns, |name| {
            closed("module type", name, ModuleType::ALL, ModuleType::name)
        })
        .map(Matcher::ModuleType),
        MatcherKind::Reference => each(patterns, regex).map(Matcher::Reference),
        MatcherKind::Regex => each(patterns, regex).map(Matcher::Regex),
        MatcherKind::String => each(patterns, regex).map(Matcher::String),
        MatcherKind::Comment => each(patterns, regex).map(Matcher::Comment),
        MatcherKind::Structural => each(patterns, structural_predicate).map(Matcher::Structural),
    }
}

/// Compiles the regexes of a `pattern` key, or returns every invalid one.
pub(super) fn regexes(patterns: Patterns) -> Result<Vec<Regex>, Vec<PatternError>> {
    each(numbered(patterns), regex)
}

/// Finds a predicate in the closed list.
pub(super) fn structural_predicate(name: &str) -> Result<StructuralPredicate, String> {
    closed(
        "structural predicate",
        name,
        StructuralPredicate::ALL,
        StructuralPredicate::name,
    )
}

/// Each pattern with its location under `pattern`.
fn numbered(patterns: Patterns) -> Vec<(String, String)> {
    match patterns {
        Patterns::One(pattern) => vec![(String::new(), pattern)],
        Patterns::Many(list) => list
            .into_iter()
            .enumerate()
            .map(|(index, pattern)| (format!("/{index}"), pattern))
            .collect(),
    }
}

fn texts(patterns: Vec<(String, String)>) -> Vec<String> {
    patterns.into_iter().map(|(_, pattern)| pattern).collect()
}

fn each<T>(
    patterns: Vec<(String, String)>,
    build: impl Fn(&str) -> Result<T, String>,
) -> Result<Vec<T>, Vec<PatternError>> {
    let mut built = Vec::with_capacity(patterns.len());
    let mut errors = Vec::new();
    for (suffix, pattern) in patterns {
        match build(&pattern) {
            Ok(value) => built.push(value),
            Err(message) => errors.push(PatternError { suffix, message }),
        }
    }
    if errors.is_empty() {
        Ok(built)
    } else {
        Err(errors)
    }
}

/// Compiles a rules regex: case-insensitive, inline flags allowed.
fn regex(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|error| match error {
            regex::Error::Syntax(detail) => {
                format!("invalid regex '{pattern}': {}", syntax_cause(&detail))
            }
            regex::Error::CompiledTooBig(limit) => {
                format!("regex '{pattern}' compiles to more than {limit} bytes")
            }
            other => format!("invalid regex '{pattern}': {other}"),
        })
}

/// The cause in a `regex` syntax error, which spans several lines.
fn syntax_cause(detail: &str) -> &str {
    detail
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("error: "))
        .unwrap_or(detail)
}

/// Finds `name` in a closed list of values.
fn closed<T: Copy, const N: usize>(
    what: &str,
    name: &str,
    all: [T; N],
    name_of: fn(T) -> &'static str,
) -> Result<T, String> {
    all.into_iter()
        .find(|value| name_of(*value) == name)
        .ok_or_else(|| {
            let expected: Vec<&str> = all.into_iter().map(name_of).collect();
            format!(
                "unknown {what} '{name}', expected one of: {}",
                expected.join(", ")
            )
        })
}
