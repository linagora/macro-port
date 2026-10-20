// SPDX-License-Identifier: Apache-2.0
//! The `macro-port` command.

#![forbid(unsafe_code)]

mod rules_check;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Inventory of Excel VBA macros, for migrations to OnlyOffice, Grist and LibreOffice Calc.
#[derive(Parser)]
#[command(name = "macro-port", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
        Command::Rules {
            command: RulesCommand::Check { rules },
        } => rules_check::run(rules.as_deref()),
    }
}

/// A `--rules` directory must exist: otherwise it is a usage error.
fn existing_dir(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_dir() {
        Ok(path)
    } else {
        Err("not an existing directory".to_owned())
    }
}
