// SPDX-License-Identifier: Apache-2.0
//! Sample reports for the tests of the renderers.
//!
//! The analysis does not exist yet, so the reports are written by hand, as the
//! JSON files of `tests/fixtures/reports/`. They fill every field that a
//! renderer reads. Each one is checked against the report schema and read into
//! the model, which denies unknown keys.

use macroport_core::report::Report;
use macroport_testkit::report_problems;

const FLEET: &str = include_str!("../fixtures/reports/fleet.json");
const INJECTION: &str = include_str!("../fixtures/reports/injection.json");

/// Eleven files, one or more for each status: the VBA projects of a workbook
/// that asks for a password, of a protected `.xls` with a form, an event
/// handler, a macro without a family and a signal of each severity, a text
/// module, a workbook with only Excel 4.0 macros, and the files that cannot
/// be read: encrypted, corrupted, too large, an archive bomb, unsupported and
/// an internal error.
pub fn fleet() -> Report {
    load("fleet.json", FLEET)
}

/// The fixture of SPEC section 8: every string that comes from an analysed
/// file (paths, names, references, snippets, error messages) carries markup, a
/// formula or the end of the page's own script.
pub fn injection() -> Report {
    load("injection.json", INJECTION)
}

/// The report in `json`, once it is valid.
fn load(name: &str, json: &str) -> Report {
    assert_eq!(report_problems(json), Vec::<String>::new(), "{name}");
    serde_json::from_str(json).unwrap_or_else(|error| panic!("{name} is a report: {error}"))
}
