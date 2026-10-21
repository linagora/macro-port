// SPDX-License-Identifier: Apache-2.0
//! The exit codes of the commands (SPEC M4). Code 0 is `ExitCode::SUCCESS`, and
//! code 2, a usage error, is the one that clap exits with.

/// The command ran to the end and found something wrong: a file that could not
/// be analysed (`scan`, `file`), or a problem in the rules (`rules check`).
pub const FAILED: u8 = 1;

/// The command could not run to the end.
pub const FATAL: u8 = 3;
