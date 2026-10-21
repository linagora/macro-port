// SPDX-License-Identifier: Apache-2.0
//! What the commands that analyse files have in common: the rules they use, the
//! words of their language, the panic hook, and how they end when something
//! fatal happens (SPEC M4: exit code 3).

use std::path::Path;
use std::process::ExitCode;

use macroport_core::report::{Lang, Report};
use macroport_core::rules::{RuleSet, RulesSources};

use crate::console::{self, say_on_stderr};
use crate::exit::{FAILED, FATAL};
use crate::i18n::Messages;
use crate::rules_check::{self, Failure};

/// Runs `body` with the rules and the words of the command, and gives the exit
/// code that it chooses.
///
/// The rules are those of the directory `rules`, once they pass the check of
/// `rules check`, or the embedded rules. If they do not pass, `body` does not
/// run: why is printed in the language of the command and the exit code is 3.
/// It is 3 too when `body` gives the text of a fatal error, which is printed in
/// the same language.
pub fn with_rules(
    lang: Lang,
    rules: Option<&Path>,
    body: impl FnOnce(&RuleSet, &Messages) -> Result<ExitCode, String>,
) -> ExitCode {
    let rules = match load_rules(rules) {
        Ok(rules) => rules,
        Err(failure) => {
            for line in failure.lines(&Messages::console(lang)) {
                say_on_stderr(&line);
            }
            return ExitCode::from(FATAL);
        }
    };
    let messages = Messages::new(lang, &rules);
    console::print_only_where_a_panic_happened(messages.text("cli-panic"));
    match body(&rules, &messages) {
        Ok(code) => code,
        Err(error) => {
            say_on_stderr(&messages.text_with("cli-error", &[("message", &error)]));
            ExitCode::from(FATAL)
        }
    }
}

/// The exit code of an analysis that ended with `report`: 1 when a file failed
/// (SPEC M4), else 0.
pub fn exit_code_of(report: &Report) -> ExitCode {
    if report.scan.files_failed > 0 {
        ExitCode::from(FAILED)
    } else {
        ExitCode::SUCCESS
    }
}

/// The rules of the directory `dir`, or the embedded rules.
fn load_rules(dir: Option<&Path>) -> Result<RuleSet, Failure> {
    match dir {
        Some(dir) => rules_check::load(Some(dir)),
        // The embedded rules skip the stage of the schemas, which `rules check`
        // runs on them: the tests do, so a build that ships does not fail here,
        // and a run does not compile three JSON Schemas to check them again.
        None => RuleSet::load(&RulesSources::EMBEDDED).map_err(Failure::CoreProblems),
    }
}
