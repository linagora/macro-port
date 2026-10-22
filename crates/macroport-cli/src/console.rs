// SPDX-License-Identifier: Apache-2.0
//! The console: the lines that a command says to the user, the document that is
//! the result of a command, and what a panic prints.
//!
//! Two policies, for two kinds of output. A line that cannot be said is lost
//! without a word: a pipe that the reader closed is no reason to lose a scan,
//! and `println!` and `eprintln!` panic on it. A document is the whole point of
//! the command that prints it, so a write that fails is an error to report,
//! unless the reader went away.

use std::io::{self, Write};
use std::panic::{self, Location};

/// Says a line on stderr.
pub fn say_on_stderr(line: &str) {
    let _ = writeln!(io::stderr().lock(), "{line}");
}

/// Says a line on stdout.
pub fn say_on_stdout(line: &str) {
    let _ = writeln!(io::stdout().lock(), "{line}");
}

/// Writes `document` on stdout, as it is: the output of a command whose result
/// is the document.
///
/// A reader that went away (`macro-port schema | head`) is no error: it
/// stopped asking for the document, and the write ends without a word.
///
/// # Errors
///
/// When stdout cannot be written, for another reason than a closed pipe.
pub fn write_document_on_stdout(document: &[u8]) -> io::Result<()> {
    write_document(&mut io::stdout().lock(), document)
}

/// Writes `document` on `out` and flushes it, and takes the error of a closed
/// pipe for a success.
fn write_document(out: &mut impl Write, document: &[u8]) -> io::Result<()> {
    match out.write_all(document).and_then(|()| out.flush()) {
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

    /// A writer that fails with `kind` when it writes, or when it is flushed.
    struct Failing {
        kind: io::ErrorKind,
        on_flush: bool,
    }

    impl Write for Failing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.on_flush {
                Ok(bytes.len())
            } else {
                Err(self.kind.into())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.on_flush {
                Err(self.kind.into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_document_is_written_as_it_is() {
        let mut out = Vec::new();

        write_document(&mut out, b"{\"a\": 1}\n").expect("a vector is written");

        assert_eq!(out, b"{\"a\": 1}\n");
    }

    #[test]
    fn a_reader_that_went_away_is_no_error() {
        for on_flush in [false, true] {
            let mut out = Failing {
                kind: io::ErrorKind::BrokenPipe,
                on_flush,
            };

            assert!(write_document(&mut out, b"x").is_ok(), "flush: {on_flush}");
        }
    }

    #[test]
    fn any_other_failure_is_an_error_of_the_same_kind() {
        let kinds = [
            io::ErrorKind::StorageFull,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ];
        for kind in kinds {
            for on_flush in [false, true] {
                let mut out = Failing { kind, on_flush };

                let error = write_document(&mut out, b"x").expect_err("the write fails");

                assert_eq!(error.kind(), kind, "flush: {on_flush}");
            }
        }
    }
}
