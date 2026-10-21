// SPDX-License-Identifier: Apache-2.0
//! `macro-port schema`: the JSON Schema of the report, the public contract of
//! the tool (SPEC M4).

use std::process::ExitCode;

use macroport_core::report::Lang;

use crate::command;
use crate::console::write_on_stdout;

/// The schema, as `schema/report.schema.json` holds it.
pub const REPORT_SCHEMA: &str = include_str!("../../../schema/report.schema.json");

/// Runs `macro-port schema`: prints the schema on stdout.
pub fn run() -> ExitCode {
    command::run(Lang::En, None, |_rules, messages| {
        write_on_stdout(REPORT_SCHEMA.as_bytes()).map_err(|error| {
            messages.text_with("cli-error-print", &[("reason", &error.to_string())])
        })?;
        Ok(ExitCode::SUCCESS)
    })
}
