// SPDX-License-Identifier: Apache-2.0
//! The CSV reports of a scan: `files.csv`, `procedures.csv` and `signals.csv`
//! (SPEC M4).
//!
//! Each document is UTF-8 with a byte order mark, which makes Excel read it as
//! UTF-8, with a comma between cells and CRLF between rows. The `csv` crate
//! quotes the cells as RFC 4180 says. The cells hold ids and numbers, never a
//! label, so the documents do not depend on the language of the report.
//!
//! A cell that a spreadsheet would read as a formula or a command, one that
//! starts with `=`, `+`, `-`, `@`, a tab or a carriage return, is prefixed with
//! `'` (SPEC section 8). That goes for every cell, since any of them may hold
//! text from an analysed file. A list in one cell is written with `;` between
//! its items, and a value that a file does not have is an empty cell.

use std::borrow::Cow;

use ::csv::{Error, Terminator, Writer, WriterBuilder};
use macroport_core::report::{FileReport, Report, Signal, Status};

use super::wire_name;

/// The UTF-8 encoding of U+FEFF.
const BYTE_ORDER_MARK: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Separates the items of a list held in one cell.
const LIST_SEPARATOR: &str = ";";

// The columns of SPEC M4, comma separated.
const FILE_COLUMNS: &str = concat!(
    "path,container,status,complexity_level,primary_family,max_severity,",
    "modules,procedures,logical_loc,xlm_present,vba_project_protected"
);
const PROCEDURE_COLUMNS: &str = concat!(
    "path,module,procedure,kind,primary_family,secondary_families,",
    "logical_loc,cyclomatic,signals,traits"
);
const SIGNAL_COLUMNS: &str = "path,module,procedure,signal_id,severity,line";

/// `files.csv`: a row for each file of `report`.
///
/// `procedures` and `logical_loc` are the file's metrics, empty for a file that
/// has none. `modules` counts those of the VBA project: 0 for a file that was
/// read and has no project, empty for a file that could not be read.
///
/// # Errors
///
/// When the CSV writer fails, which writing to memory does not do.
pub fn files(report: &Report) -> Result<Vec<u8>, Error> {
    let mut table = Table::new(FILE_COLUMNS)?;
    for file in &report.files {
        let metrics = file.metrics.as_ref();
        table.row(&[
            file.path.clone(),
            wire_name(&file.container),
            wire_name(&file.status),
            optional(file.complexity_level.as_ref().map(wire_name)),
            optional(file.primary_family.as_ref()),
            optional(file.max_severity.as_ref().map(wire_name)),
            optional(module_count(file)),
            optional(metrics.and_then(|metrics| metrics.procedures)),
            optional(metrics.map(|metrics| metrics.logical_loc)),
            optional(file.xlm.as_ref().map(|xlm| xlm.present)),
            optional(file.vba_project.as_ref().map(|project| project.protected)),
        ])?;
    }
    table.finish()
}

/// The modules in the VBA project of `file`: none when the file was read and
/// has no project, unknown when it could not be read.
fn module_count(file: &FileReport) -> Option<usize> {
    match (&file.vba_project, file.status) {
        (Some(project), _) => Some(project.modules.len()),
        (None, Status::Ok | Status::NoMacros) => Some(0),
        (None, _) => None,
    }
}

/// `procedures.csv`: a row for each procedure of each module of each file of
/// `report`.
///
/// The ids of the procedure's secondary families, signals and traits each go
/// in one cell, separated by `;`, in the order of the report.
///
/// # Errors
///
/// When the CSV writer fails, which writing to memory does not do.
pub fn procedures(report: &Report) -> Result<Vec<u8>, Error> {
    let mut table = Table::new(PROCEDURE_COLUMNS)?;
    for file in &report.files {
        let Some(project) = &file.vba_project else {
            continue;
        };
        for module in &project.modules {
            for procedure in &module.procedures {
                table.row(&[
                    file.path.clone(),
                    module.name.clone(),
                    procedure.name.clone(),
                    wire_name(&procedure.kind),
                    procedure.primary_family.clone(),
                    list_cell(
                        procedure
                            .secondary_families
                            .iter()
                            .flatten()
                            .map(String::as_str),
                    ),
                    procedure.metrics.logical_loc.to_string(),
                    optional(procedure.metrics.cyclomatic),
                    list_cell(
                        procedure
                            .signals
                            .iter()
                            .map(|signal| signal.signal_id.as_str()),
                    ),
                    list_cell(procedure.traits.iter().flatten().map(String::as_str)),
                ])?;
            }
        }
    }
    table.finish()
}

/// `signals.csv`: a row for each line of evidence of each signal of `report`,
/// or one row for a signal that has none, with an empty `line`.
///
/// A line counts once for a signal. A signal that does not name its module or
/// procedure takes the one it was found in; a file's own signals have none.
/// Within a file come first its own signals, then each module's followed by
/// those of its procedures.
///
/// A row is a signal at a line, not a finding. The evidence of a chain is the
/// lines of its terms, which have their own rows under their own ids. To count
/// the findings, count the distinct `path`, `module`, `procedure` and
/// `signal_id`.
///
/// # Errors
///
/// When the CSV writer fails, which writing to memory does not do.
pub fn signals(report: &Report) -> Result<Vec<u8>, Error> {
    let mut table = Table::new(SIGNAL_COLUMNS)?;
    for file in &report.files {
        let in_file = Place {
            path: &file.path,
            module: None,
            procedure: None,
        };
        for signal in file.signals.iter().flatten() {
            signal_rows(&mut table, in_file, signal)?;
        }
        let Some(project) = &file.vba_project else {
            continue;
        };
        for module in &project.modules {
            let in_module = Place {
                module: Some(&module.name),
                ..in_file
            };
            for signal in module.signals.iter().flatten() {
                signal_rows(&mut table, in_module, signal)?;
            }
            for procedure in &module.procedures {
                let in_procedure = Place {
                    procedure: Some(&procedure.name),
                    ..in_module
                };
                for signal in &procedure.signals {
                    signal_rows(&mut table, in_procedure, signal)?;
                }
            }
        }
    }
    table.finish()
}

/// Where a signal was found.
#[derive(Clone, Copy)]
struct Place<'a> {
    path: &'a str,
    module: Option<&'a str>,
    procedure: Option<&'a str>,
}

/// Writes the rows of `signal`, found at `place`.
fn signal_rows(table: &mut Table, place: Place<'_>, signal: &Signal) -> Result<(), Error> {
    let module = signal.module.as_deref().or(place.module);
    let procedure = signal.procedure.as_deref().or(place.procedure);
    let severity = wire_name(&signal.severity);
    let mut lines: Vec<String> = Vec::new();
    for entry in signal.evidence.iter().flatten() {
        let line = entry.line.to_string();
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    for line in &lines {
        table.row(&[
            place.path,
            module.unwrap_or_default(),
            procedure.unwrap_or_default(),
            &signal.signal_id,
            &severity,
            line,
        ])?;
    }
    Ok(())
}

/// The text of `value`, or an empty cell when there is none.
fn optional<T: ToString>(value: Option<T>) -> String {
    value.as_ref().map(ToString::to_string).unwrap_or_default()
}

/// `items` in one cell.
fn list_cell<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.collect::<Vec<_>>().join(LIST_SEPARATOR)
}

/// A CSV document being written: the byte order mark, then the header and the
/// rows.
struct Table {
    writer: Writer<Vec<u8>>,
}

impl Table {
    /// A table with the comma separated `columns`.
    fn new(columns: &str) -> Result<Self, Error> {
        let mut table = Self {
            writer: WriterBuilder::new()
                .terminator(Terminator::CRLF)
                .from_writer(BYTE_ORDER_MARK.to_vec()),
        };
        table.row(&columns.split(',').collect::<Vec<_>>())?;
        Ok(table)
    }

    /// Writes a row, each cell protected. The writer refuses a row of another
    /// length than the header.
    fn row<S: AsRef<str>>(&mut self, cells: &[S]) -> Result<(), Error> {
        let cells: Vec<Cow<'_, str>> = cells.iter().map(|cell| protect(cell.as_ref())).collect();
        self.writer
            .write_record(cells.iter().map(|cell| cell.as_bytes()))
    }

    /// The document.
    fn finish(self) -> Result<Vec<u8>, Error> {
        self.writer
            .into_inner()
            .map_err(|error| Error::from(error.into_error()))
    }
}

/// `cell`, prefixed with `'` when a spreadsheet would read it as a formula or
/// a command.
fn protect(cell: &str) -> Cow<'_, str> {
    if cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        Cow::Owned(format!("'{cell}"))
    } else {
        Cow::Borrowed(cell)
    }
}
