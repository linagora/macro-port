// SPDX-License-Identifier: Apache-2.0
//! What the commands that analyse files have in common: the rules they use, the
//! words of their language, the panic hook, and how they end when something
//! fatal happens (SPEC M4: exit code 3).

use std::path::Path;
use std::process::ExitCode;

use macroport_core::report::{Lang, Report};
use macroport_core::rules::{RuleSet, RulesSources};

use crate::console::{self, say_on_stderr};
use crate::i18n::Messages;
use crate::rules_check;

/// Exit code of an analysis in which at least one file failed (SPEC M4).
pub const FAILED_FILES: u8 = 1;

/// Exit code of a fatal error (SPEC M4).
pub const FATAL: u8 = 3;

/// Runs `body` with the rules and the words of the command, and gives the exit
/// code that it chooses.
///
/// The rules are those of the directory `rules`, once they pass the check of
/// `rules check`, or the embedded rules. If they do not pass, `body` does not
/// run: the problems are printed and the exit code is 3. It is 3 too when
/// `body` gives the text of a fatal error, which is printed in the language of
/// the command.
pub fn run(
    lang: Lang,
    rules: Option<&Path>,
    body: impl FnOnce(&RuleSet, &Messages) -> Result<ExitCode, String>,
) -> ExitCode {
    let rules = match load_rules(rules) {
        Ok(rules) => rules,
        Err(code) => return code,
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
        ExitCode::from(FAILED_FILES)
    } else {
        ExitCode::SUCCESS
    }
}

/// The rules of the directory `dir`, or the embedded rules. `Err` is the exit
/// code of a command that cannot go on, once it has said why.
fn load_rules(dir: Option<&Path>) -> Result<RuleSet, ExitCode> {
    match dir {
        Some(dir) => rules_check::load(Some(dir)).map_err(|failure| {
            failure.print();
            ExitCode::from(FATAL)
        }),
        None => RuleSet::load(&RulesSources::EMBEDDED).map_err(|problems| {
            // The tests check the embedded rules, so a build that ships does
            // not get here. The messages take the labels of the rules, so
            // without them there is no Fluent text to print.
            let first = problems.first().map(ToString::to_string);
            say_on_stderr(&format!(
                "error: the embedded rules do not load: {}",
                first.unwrap_or_default()
            ));
            ExitCode::from(FATAL)
        }),
    }
}
