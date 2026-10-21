// SPDX-License-Identifier: Apache-2.0
//! The files that make up each format of the reports: which renderer writes
//! which file (SPEC M4).

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

impl Document {
    fn new(name: &'static str, contents: Vec<u8>) -> Self {
        Self { name, contents }
    }
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
        Format::Json => {
            let json = report.to_json().map_err(|error| error.to_string())?;
            Ok(vec![Document::new("report.json", json.into_bytes())])
        }
        Format::Html => {
            let page = html::render(report, messages).map_err(|error| error.to_string())?;
            Ok(vec![Document::new("report.html", page.into_bytes())])
        }
        Format::Csv => Ok(vec![
            Document::new("files.csv", csv::files(report).map_err(|e| e.to_string())?),
            Document::new(
                "procedures.csv",
                csv::procedures(report).map_err(|e| e.to_string())?,
            ),
            Document::new(
                "signals.csv",
                csv::signals(report).map_err(|e| e.to_string())?,
            ),
        ]),
    }
}
