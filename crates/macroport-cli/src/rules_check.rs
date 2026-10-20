// SPDX-License-Identifier: Apache-2.0
//! `macro-port rules check`: each rules file is validated against its JSON
//! Schema; once every file passes, the core loads the rules, which compiles
//! the regexes and rejects duplicate ids.

use std::cmp::Ordering;
use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use jsonschema::error::ValidationErrorKind;
use macroport_core::rules::{self, FAMILIES_FILE, Problem, RuleSet, RulesSources};
use serde_json::Value;

const FAMILIES_SCHEMA: &str = include_str!("../../../schema/rules/families.schema.json");

/// Rules files above this size are refused.
const MAX_FILE_MIB: u64 = 1;

/// Exit code when the rules have problems.
const PROBLEMS_FOUND: u8 = 1;

/// Exit code of a fatal error (SPEC M4).
const FATAL: u8 = 3;

/// How a check ends.
enum Outcome {
    /// Every rules file is valid.
    Valid,
    /// Problems with a rules file, or found by the core.
    Problems(Vec<Problem>),
    /// Problems found by the schemas: the core has not loaded the rules yet.
    SchemaProblems(Vec<Problem>),
    /// The check could not run.
    Fatal(String),
}

/// Checks the rules of `dir`, or the embedded rules, and prints the outcome.
pub fn run(dir: Option<&Path>) -> ExitCode {
    match check(dir) {
        Outcome::Valid => {
            println!("{FAMILIES_FILE}: ok");
            ExitCode::SUCCESS
        }
        Outcome::Problems(problems) => print_problems(&problems, ""),
        Outcome::SchemaProblems(problems) => print_problems(
            &problems,
            " (regexes and duplicate ids are checked once every file matches its schema)",
        ),
        Outcome::Fatal(message) => {
            eprintln!("error: {message}");
            ExitCode::from(FATAL)
        }
    }
}

fn check(dir: Option<&Path>) -> Outcome {
    let families = match dir.map(|dir| read(dir, FAMILIES_FILE)).transpose() {
        Ok(families) => families,
        Err(outcome) => return outcome,
    };
    let sources = match &families {
        Some(families) => RulesSources {
            families,
            ..RulesSources::EMBEDDED
        },
        None => RulesSources::EMBEDDED,
    };
    let document = match parse(FAMILIES_FILE, sources.families) {
        Ok(document) => document,
        Err(problem) => return Outcome::Problems(vec![problem]),
    };
    match validate(FAMILIES_FILE, &document, FAMILIES_SCHEMA) {
        Ok(problems) if problems.is_empty() => {}
        Ok(problems) => return Outcome::SchemaProblems(problems),
        Err(message) => return Outcome::Fatal(message),
    }
    match RuleSet::load(&sources) {
        Ok(_) => Outcome::Valid,
        Err(problems) => Outcome::Problems(problems),
    }
}

/// Prints `problems`, then their count followed by `note`.
fn print_problems(problems: &[Problem], note: &str) -> ExitCode {
    for problem in problems {
        eprintln!("{problem}");
    }
    let count = problems.len();
    let noun = if count == 1 { "problem" } else { "problems" };
    eprintln!("{count} {noun} found{note}");
    ExitCode::from(PROBLEMS_FOUND)
}

/// Parses one rules file. The core reports the same YAML errors, but without
/// saying that they are YAML errors.
fn parse(file: &'static str, text: &str) -> Result<Value, Problem> {
    match rules::parse_value(text) {
        Ok(Value::Null) => Err(Problem::in_file(file, "empty document".to_owned())),
        Ok(document) => Ok(document),
        Err(message) => Err(Problem::in_file(file, format!("invalid YAML: {message}"))),
    }
}

/// Validates a rules document against its JSON Schema. `Err` when the
/// embedded schema itself is broken.
fn validate(file: &'static str, document: &Value, schema: &str) -> Result<Vec<Problem>, String> {
    let broken = |error: &dyn Display| format!("the embedded schema of {file} is invalid: {error}");
    let schema: Value = serde_json::from_str(schema).map_err(|error| broken(&error))?;
    let validator = jsonschema::options()
        .build(&schema)
        .map_err(|error| broken(&error))?;
    let mut problems = Vec::new();
    for error in validator.iter_errors(document) {
        let at = error.instance_path().as_str();
        let found: Vec<(String, String)> = match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
                .iter()
                .map(|key| {
                    let location = format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"));
                    (location, format!("unknown key '{key}'"))
                })
                .collect(),
            ValidationErrorKind::Required { property } => {
                vec![(at.to_owned(), format!("missing key '{}'", plain(property)))]
            }
            ValidationErrorKind::Enum { options } => {
                let expected: Vec<String> = options
                    .as_array()
                    .map(|options| options.iter().map(plain).collect())
                    .unwrap_or_default();
                let message = format!(
                    "'{}' is not one of: {}",
                    plain(error.instance()),
                    expected.join(", ")
                );
                vec![(at.to_owned(), message)]
            }
            _ => vec![(at.to_owned(), error.to_string())],
        };
        problems.extend(found.into_iter().map(|(location, message)| Problem {
            file,
            rule: rule_at(document, &location),
            location,
            message,
        }));
    }
    problems.sort_by(|a, b| {
        pointer_order(&a.location, &b.location).then_with(|| a.message.cmp(&b.message))
    });
    Ok(problems)
}

/// A JSON value as text, strings without their quotes.
fn plain(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// The id of the deepest object on the way to `location` that has one.
fn rule_at(document: &Value, location: &str) -> Option<String> {
    let mut node = document;
    let mut rule = None;
    for segment in location.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        let next = match node {
            Value::Object(map) => map.get(&segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        };
        let Some(next) = next else { break };
        node = next;
        if let Some(id) = node.get("id").and_then(Value::as_str) {
            rule = Some(id.to_owned());
        }
    }
    rule
}

/// Orders JSON Pointers segment by segment, array indexes as numbers and keys
/// alphabetically: a stable order, though not always the file's.
fn pointer_order(a: &str, b: &str) -> Ordering {
    let segments = |pointer: &str| -> Vec<(Option<u64>, String)> {
        pointer
            .split('/')
            .map(|segment| (segment.parse().ok(), segment.to_owned()))
            .collect()
    };
    segments(a).cmp(&segments(b))
}

/// Reads one rules file of `dir`, or returns how the check ends.
fn read(dir: &Path, file: &'static str) -> Result<String, Outcome> {
    let path = dir.join(file);
    let max_bytes = MAX_FILE_MIB << 20;
    let mut bytes = Vec::new();
    File::open(&path)
        .and_then(|handle| handle.take(max_bytes + 1).read_to_end(&mut bytes))
        .map_err(|error| Outcome::Fatal(format!("cannot read {}: {error}", path.display())))?;
    let problem = |message: String| Outcome::Problems(vec![Problem::in_file(file, message)]);
    if bytes.len() as u64 > max_bytes {
        return Err(problem(format!("larger than {MAX_FILE_MIB} MiB")));
    }
    String::from_utf8(bytes).map_err(|error| problem(format!("not UTF-8: {error}")))
}

#[cfg(test)]
mod tests {
    use macroport_core::rules::{ModuleType, StructuralPredicate};

    use super::*;

    fn schema() -> Value {
        serde_json::from_str(FAMILIES_SCHEMA).expect("the schema is JSON")
    }

    fn names(enumeration: &Value) -> Vec<String> {
        enumeration["enum"]
            .as_array()
            .expect("an enum")
            .iter()
            .map(plain)
            .collect()
    }

    /// The closed lists of the schema are the ones the core knows.
    #[test]
    fn closed_lists_match_the_core() {
        let schema = schema();

        let predicates: Vec<&str> = StructuralPredicate::ALL
            .into_iter()
            .map(StructuralPredicate::name)
            .collect();
        assert_eq!(names(&schema["$defs"]["structural_predicate"]), predicates);

        let module_types: Vec<&str> = ModuleType::ALL.into_iter().map(ModuleType::name).collect();
        assert_eq!(names(&schema["$defs"]["module_type"]), module_types);
    }

    /// Every matcher, scope and text mode that the schema allows loads in the core.
    #[test]
    fn the_core_loads_every_keyword_of_the_schema() {
        let schema = schema();
        let signal = &schema["$defs"]["signal"]["properties"];
        let mut signals = Vec::new();
        for matcher in names(&signal["match"]) {
            let pattern = match matcher.as_str() {
                "structural" => "udf_candidate",
                "module_type" => "standard",
                _ => "x",
            };
            signals.push(format!("match: {matcher}, pattern: {pattern}"));
        }
        for scope in names(&signal["scope"]) {
            signals.push(format!("match: member, pattern: x, scope: {scope}"));
        }
        for text in names(&signal["text"]) {
            signals.push(format!("match: member, pattern: x, text: {text}"));
        }

        for signal in signals {
            let families = format!(
                "version: 0.1.0\n\
                 scoring: {{ min_score: 3, secondary_ratio: 0.6, default_max_hits_per_procedure: 3 }}\n\
                 families:\n  \
                   - {{ id: 01-x, corpus_slug: 01-x, label: {{ en: X, fr: X }}, \
                        signals: [{{ id: s, weight: 1, {signal} }}] }}\n"
            );
            let document = parse(FAMILIES_FILE, &families).expect("valid YAML");
            assert_eq!(
                validate(FAMILIES_FILE, &document, FAMILIES_SCHEMA),
                Ok(Vec::new()),
                "{signal}"
            );
            let loaded = RuleSet::load(&RulesSources {
                families: &families,
                ..RulesSources::EMBEDDED
            });
            assert!(loaded.is_ok(), "{signal}: {loaded:?}");
        }
    }
}
