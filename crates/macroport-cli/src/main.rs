// SPDX-License-Identifier: Apache-2.0
//! The `macro-port` command.

#![forbid(unsafe_code)]

use clap::Parser;

/// Inventory of Excel VBA macros, for migrations to OnlyOffice, Grist and LibreOffice Calc.
#[derive(Parser)]
#[command(name = "macro-port", version)]
struct Cli {}

fn main() {
    Cli::parse();
}
