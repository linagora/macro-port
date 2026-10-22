// SPDX-License-Identifier: Apache-2.0
//! The `macro-port` command.

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use macroport_cli::args::existing_dir;
use macroport_cli::{rules_check, scan};

/// Inventory of Excel VBA macros, for migrations to OnlyOffice, Grist and LibreOffice Calc.
#[derive(Parser)]
#[command(name = "macro-port", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyse the Excel files and VBA modules under some paths, and write the reports.
    Scan(scan::Args),
    /// Work with the rules files.
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
}

#[derive(Subcommand)]
enum RulesCommand {
    /// Validate the rules: schemas, regexes, ids and references.
    Check {
        /// Directory holding the rules files to use instead of the embedded ones.
        #[arg(long, value_name = "DIR", value_parser = existing_dir)]
        rules: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Scan(args) => scan::run(&args),
        Command::Rules {
            command: RulesCommand::Check { rules },
        } => rules_check::run(rules.as_deref()),
    }
}
