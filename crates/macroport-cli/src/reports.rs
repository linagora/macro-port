// SPDX-License-Identifier: Apache-2.0
//! The files that make up each format of the reports: which renderer writes
//! which file (SPEC M4).

use std::fmt::Display;

use macroport_core::report::Report;

use crate::args::Format;
use crate::i18n::Messages;
use crate::render::{csv, html};

/// A file of the reports: its name in the output directory and its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub name: &'static str,
    pub contents: Vec<u8>,
}

/// The files of `report` in `format`, in the language of `messages`.
///
/// # Errors
///
/// The text of the error of a renderer, which the model of the report never
/// raises.
pub fn documents(
    format: Format,
    report: &Report,
    messages: &Messages,
) -> Result<Vec<Document>, String> {
    match format {
        Format::Json => Ok(vec![document("report.json", report.to_json())?]),
        Format::Html => Ok(vec![document(
            "report.html",
            html::render(report, messages),
        )?]),
        Format::Csv => Ok(vec![
            document("files.csv", csv::files(report))?,
            document("procedures.csv", csv::procedures(report))?,
            document("signals.csv", csv::signals(report))?,
        ]),
    }
}

/// The file `name` that a renderer made, or the text of its error.
fn document(
    name: &'static str,
    rendered: Result<impl Into<Vec<u8>>, impl Display>,
) -> Result<Document, String> {
    match rendered {
        Ok(contents) => Ok(Document {
            name,
            contents: contents.into(),
        }),
        Err(error) => Err(error.to_string()),
    }
}
