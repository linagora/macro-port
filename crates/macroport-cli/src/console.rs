// SPDX-License-Identifier: Apache-2.0
//! The console: the lines that a command prints for the user, and what a panic
//! prints.
//!
//! A line that cannot be printed is lost without a word. A pipe that the reader
//! closed is no reason to lose a scan, and `println!` and `eprintln!` panic on
//! it.

use std::io::{self, Write};
use std::panic::{self, Location};

/// Prints a line on stderr.
pub fn say_on_stderr(line: &str) {
    let _ = writeln!(io::stderr().lock(), "{line}");
}

/// Prints a line on stdout.
pub fn say_on_stdout(line: &str) {
    let _ = writeln!(io::stdout().lock(), "{line}");
}

/// Writes `bytes` on stdout, as they are: the output of a command whose result
/// is the document, where a write that fails is an error to report.
///
/// A reader that went away (`macro-port schema | head`) is not one: it
/// stopped asking for the document, and the write ends without a word.
///
/// # Errors
///
/// When stdout cannot be written, for another reason than a closed pipe.
pub fn write_on_stdout(bytes: &[u8]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    match stdout.write_all(bytes).and_then(|()| stdout.flush()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        written => written,
    }
}

/// Makes a panic print `label` and the place where it happened, and nothing
/// else.
///
/// The message of a panic may quote the file that was being analysed, and a
/// file is hostile: its text must not reach the console, where control
/// characters would act. The place is a line of the program, enough to report
/// the bug.
pub fn print_only_where_a_panic_happened(label: String) {
    panic::set_hook(Box::new(move |info| {
        say_on_stderr(&panic_line(&label, info.location()));
    }));
}

/// The line that a panic prints.
fn panic_line(label: &str, location: Option<&Location<'_>>) -> String {
    match location {
        Some(location) => format!("{label} {location}"),
        None => label.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_line_is_the_label_and_the_place() {
        let here = Location::caller();

        let line = panic_line("internal error at", Some(here));

        let place = format!("{}:{}:{}", here.file(), here.line(), here.column());
        assert_eq!(line, format!("internal error at {place}"));
        assert!(line.contains("console.rs"), "{line}");
    }

    #[test]
    fn a_panic_with_no_place_is_the_label() {
        assert_eq!(panic_line("internal error at", None), "internal error at");
    }
}
