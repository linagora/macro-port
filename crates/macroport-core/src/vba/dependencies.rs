// SPDX-License-Identifier: Apache-2.0
//! The external dependencies of a file (`rules/complexity.yaml`): the ProgIDs
//! its code passes to `CreateObject` and `GetObject`, and the references of its
//! project that every project of Excel does not have. The third kind, the
//! `Declare` statements, is for the caller to count.
//!
//! The lists below are written here and not in the rules: the file gives them
//! in a comment, and its `metric_definitions` are documentation (SPEC section 7).

use std::collections::BTreeSet;

/// The ProgIDs that are no external dependency (`rules/complexity.yaml`): the
/// scripting runtime is part of every Windows.
const COMMON_PROGIDS: [&str; 2] = ["Scripting.Dictionary", "VBScript.RegExp"];

/// The longest string that is taken for a ProgID. A folded expression can make
/// a string of megabytes, which is turned down by its length alone.
const MAX_PROGID_BYTES: usize = 255;

/// The display names, in lower case, of the references that every project of
/// Excel has. `rules/complexity.yaml` calls default the references VBA, Excel,
/// stdole and Office. A project names a reference by the registered name of its
/// type library, as `schema/examples/report.example.json` lists them: Visual
/// Basic For Applications, OLE Automation, and the object libraries of Excel
/// and of Office, whose names carry a version (see [`is_default_reference`]).
/// The short names of the YAML are kept as well.
const DEFAULT_REFERENCES: [&str; 6] = [
    "visual basic for applications",
    "ole automation",
    "vba",
    "stdole",
    "excel",
    "office",
];

/// The number of distinct ProgIDs among `progids`, the strings that some code
/// passes to `CreateObject` and `GetObject`, after constant folding. Two that
/// differ by their letter case are one. `Scripting.Dictionary` and
/// `VBScript.RegExp` do not count, and neither does a string that is not
/// shaped like a ProgID (see [`is_progid`]). The procedures count their
/// dependencies the same way (`analyze::metrics`).
pub(crate) fn count_progids<I>(progids: I) -> u32
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let distinct: BTreeSet<String> = progids
        .into_iter()
        .filter(|progid| is_progid(progid.as_ref()) && !is_common_progid(progid.as_ref()))
        .map(|progid| progid.as_ref().to_ascii_lowercase())
        .collect();
    u32::try_from(distinct.len()).unwrap_or(u32::MAX)
}

/// Whether `text` is shaped like a ProgID: a letter, then letters, digits, `_`
/// and `.`, with a `.` that does not end it, as in `Word.Application.16`, and
/// at most 255 bytes long. A path (`C:\Reports\book.xlsx`) or a moniker
/// (`winmgmts:`) that `GetObject` takes has characters that no ProgID has, and
/// so does anything with a space. A ProgID with no `.`, such as `htmlfile`, is
/// not counted.
///
/// The shape is all that is tested: a bare file name such as `book.xlsx` has
/// it. The strings to give are the arguments of `CreateObject` and `GetObject`,
/// not every string of the statements that call them.
fn is_progid(text: &str) -> bool {
    text.len() <= MAX_PROGID_BYTES
        && text
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
        && text.contains('.')
        && !text.ends_with('.')
}

fn is_common_progid(progid: &str) -> bool {
    COMMON_PROGIDS
        .iter()
        .any(|common| common.eq_ignore_ascii_case(progid))
}

/// How many distinct references among `references`, the display names of the
/// references of a project, are not default ones. A reference is told by its
/// display name, whatever its case and its margins; a blank one counts for
/// nothing.
pub(super) fn count_references(references: &[String]) -> u32 {
    let distinct: BTreeSet<String> = references
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty() && !is_default_reference(name))
        .collect();
    u32::try_from(distinct.len()).unwrap_or(u32::MAX)
}

/// Whether the display name of a reference, in lower case, is one of the four
/// that every project of Excel has: Visual Basic for Applications, OLE
/// Automation, and the object libraries of Excel and of Office, which have a
/// version in their name (`microsoft excel 16.0 object library`). The short
/// names of the YAML are default too.
fn is_default_reference(name: &str) -> bool {
    let versioned = ["microsoft excel ", "microsoft office "]
        .into_iter()
        .filter_map(|prefix| name.strip_prefix(prefix))
        .filter_map(|rest| rest.strip_suffix(" object library"))
        .any(is_version);
    versioned || DEFAULT_REFERENCES.contains(&name)
}

/// Whether `text` is a version number: digits and dots, the first a digit.
fn is_version(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit())
        && text.chars().all(|c| c.is_ascii_digit() || c == '.')
}
