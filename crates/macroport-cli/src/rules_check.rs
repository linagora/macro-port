// SPDX-License-Identifier: Apache-2.0
//! `macro-port rules check`: the core loads each rules file, which compiles
//! the regexes and rejects duplicate ids.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use macroport_core::rules::{self, FAMILIES_FILE, Problem, RuleSet, RulesSources};
use serde_json::Value;

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
        Outcome::Problems(problems) => print_problems(&problems),
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
        Some(families) => RulesSources { families },
        None => RulesSources::EMBEDDED,
    };
    if let Err(problem) = parse(FAMILIES_FILE, sources.families) {
        return Outcome::Problems(vec![problem]);
    }
    match RuleSet::load(&sources) {
        Ok(_) => Outcome::Valid,
        Err(problems) => Outcome::Problems(problems),
    }
}

/// Prints `problems`, then their count.
fn print_problems(problems: &[Problem]) -> ExitCode {
    for problem in problems {
        eprintln!("{problem}");
    }
    let count = problems.len();
    let noun = if count == 1 { "problem" } else { "problems" };
    eprintln!("{count} {noun} found");
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
