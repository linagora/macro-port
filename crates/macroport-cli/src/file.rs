// SPDX-License-Identifier: Apache-2.0
//! `macro-port file`: one file analysed, and its report printed on stdout (SPEC
//! M4). It is the report of a scan of that file: the same JSON, the same HTML
//! page, with the one file in them.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args as ClapArgs;
use macroport_core::rules::RuleSet;

use crate::args::{AnalysisArgs, PrintFormat, existing_file};
use crate::command;
use crate::i18n::Messages;
use crate::reports;
use crate::scan::{self, Settings};

/// The arguments of `macro-port file`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The file to analyse, whatever its extension.
    #[arg(value_name = "FILE", value_parser = existing_file)]
    pub path: PathBuf,

    /// The report to print on stdout: `json` (the report that the schema
    /// describes) or `html` (one page that holds its data and its style).
    #[arg(long, value_enum, default_value_t, value_name = "FORMAT")]
    pub format: PrintFormat,

    #[command(flatten)]
    pub analysis: AnalysisArgs,
}

/// Runs `macro-port file`.
pub fn run(args: &Args) -> ExitCode {
    let analysis = &args.analysis;
    command::with_rules(
        analysis.lang(),
        analysis.rules.as_deref(),
        |rules, messages| execute(args, rules, messages),
    )
}

/// The analysis and the report on stdout, or the fatal error that stopped them.
fn execute(args: &Args, rules: &RuleSet, messages: &Messages) -> Result<ExitCode, String> {
    // One file: one thread, and no progress to show.
    let settings = Settings::new(rules, Some(1), &args.analysis);
    let report = scan::scan(std::slice::from_ref(&args.path), &settings, &|_| {})
        .map_err(|error| scan::threads_error(messages, &error))?;
    let document = reports::printed(args.format, &report, messages)?;
    command::print_document(messages, &document)?;
    Ok(command::exit_code_of(&report))
}
