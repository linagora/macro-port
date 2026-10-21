// SPDX-License-Identifier: Apache-2.0
//! The HTML report of a scan: `report.html`, one self-contained file (SPEC M4).
//!
//! The page makes no request and runs nothing that comes from an analysed file
//! (SPEC section 8, A7). Every string of the report is untrusted, and it
//! reaches the page in one of two ways:
//!
//! - As text of the page, through an expression of the template
//!   (`templates/report.html`), which askama escapes. The template applies no
//!   filter, and the words of the page come from [`Messages`].
//! - Within the whole report, embedded as JSON in a `<script
//!   type="application/json">` block, for the tools that read the page. HTML
//!   escaping would be wrong there, so `<`, `>`, `&`, U+2028 and U+2029 are
//!   written as `\u` escapes instead, and neither `</script` nor `<!--` can
//!   occur in the block.
//!
//! That block and the page's own stylesheet and script, files of this crate,
//! are the only values that escape no character. Each is a [`Safe`] field of
//! `Page`, so the template needs no `safe` filter, and a test counts the uses
//! of [`Safe`].
//!
//! The script filters the list of files. The page works without it: the toolbar
//! stays hidden until the script shows it, and the list is the one that the
//! template wrote. The script reads the text of the page and sets properties of
//! its elements, and it writes no markup.
//!
//! A `<meta>` policy allows the one inline `<style>` and the one inline
//! `<script>`, each by its SHA-256 hash, and data URIs for images, and nothing
//! else, so no other script would run and none could connect. It is the first
//! element after the character set, and it also forbids `<base>` and forms.

use std::borrow::Cow;
use std::collections::BTreeMap;

use askama::Template;
use askama::filters::Safe;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use macroport_core::report::{Evidence, FamilyScore, FileError, FileReport, Report, Signal};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::i18n::Messages;
use crate::wire::wire_name;

/// The page's stylesheet and script, in files so that editors know what they
/// are. Their hashes are in the policy of the page.
const STYLE: &str = include_str!("../../templates/report.css");
const SCRIPT: &str = include_str!("../../templates/report.js");

/// The HTML report of `report`, in the language of `messages`.
///
/// # Errors
///
/// When the template fails or the report cannot be written as JSON, neither of
/// which this model does.
pub fn render(report: &Report, messages: &Messages) -> Result<String, askama::Error> {
    let style = lf(STYLE);
    let script = lf(SCRIPT);
    Page {
        messages,
        report,
        lang: wire_name(&messages.lang()),
        policy: policy(&style, &script),
        style: Safe(&style),
        script: Safe(&script),
        data: Safe(data_block(report).map_err(askama::Error::custom)?),
        roots: report.scan.roots.as_deref().unwrap_or_default(),
        counts: counts(report, messages),
        distributions: distributions(report, messages),
        files: file_views(report, messages),
    }
    .render()
}

#[derive(Template)]
#[template(path = "report.html")]
struct Page<'a> {
    messages: &'a Messages,
    report: &'a Report,
    /// `en` or `fr`.
    lang: String,
    policy: String,
    /// The page's stylesheet and script, files of this crate: not escaped.
    style: Safe<&'a str>,
    script: Safe<&'a str>,
    /// The report as JSON that [`data_block`] made safe to put in a `<script>`
    /// element: not escaped for HTML.
    data: Safe<String>,
    /// The paths that were scanned, none when the report does not say.
    roots: &'a [String],
    counts: Vec<Count>,
    distributions: Vec<Distribution>,
    files: Vec<FileView<'a>>,
}

/// A number and what it counts.
struct Count {
    label: String,
    value: u32,
}

/// The files counted by one of their properties.
struct Distribution {
    title: String,
    rows: Vec<Count>,
    /// What a full bar stands for.
    max: u32,
}

/// A file with the words of its format and chips, its families, signals and
/// traits with their labels, and the lists of its report that are empty when it
/// has none.
struct FileView<'a> {
    file: &'a FileReport,
    container: String,
    chips: Vec<Chip>,
    references: &'a [String],
    errors: &'a [FileError],
    notes: &'a [String],
    traits: Vec<String>,
    families: Vec<FamilyView<'a>>,
    signals: Vec<SignalView<'a>>,
}

/// A word shown as a badge, coloured after the value it names when it names one.
struct Chip {
    /// What the word is about, shown before it.
    hint: Option<String>,
    label: String,
    /// The whole `class` attribute: `chip`, or `chip status-ok`,
    /// `chip level-low`, `chip severity-high`, with the kind of value and its
    /// name in the schema, never a string of an analysed file.
    class: String,
}

impl Chip {
    /// A chip coloured after `value`, a value of the enumeration `kind` of the
    /// schema.
    fn new<T: Serialize>(
        hint: Option<String>,
        kind: &'static str,
        value: &T,
        label: String,
    ) -> Self {
        Self {
            hint,
            label,
            class: format!("chip {kind}-{}", wire_name(value)),
        }
    }
}

/// A usage family with its score and the lines that raised it.
struct FamilyView<'a> {
    label: String,
    /// The score as the language writes a number.
    score: String,
    evidence: &'a [Evidence],
}

/// A security signal with its label and severity, where it was raised and the
/// lines that raised it.
struct SignalView<'a> {
    id: &'a str,
    label: String,
    severity: Chip,
    /// `Module.Procedure`, or the one of the two that the report names.
    place: Option<String>,
    evidence: &'a [Evidence],
}

/// The counts of the scan, then those of the summary that the report has.
fn counts(report: &Report, messages: &Messages) -> Vec<Count> {
    let scan = &report.scan;
    let sums = report.summary.totals.as_ref();
    [
        ("html-files", Some(scan.files_total)),
        ("html-files-with-macros", Some(scan.files_with_macros)),
        ("html-files-failed", Some(scan.files_failed)),
        ("html-modules", sums.and_then(|totals| totals.modules)),
        ("html-procedures", sums.and_then(|totals| totals.procedures)),
        (
            "html-logical-loc",
            sums.and_then(|totals| totals.logical_loc),
        ),
        (
            "html-possibly-unused",
            sums.and_then(|totals| totals.possibly_unused_procedures),
        ),
    ]
    .into_iter()
    .filter_map(|(id, value)| {
        Some(Count {
            label: messages.text(id),
            value: value?,
        })
    })
    .collect()
}

/// The files by status, by complexity level and by highest severity, then the
/// procedures by family. A bar shows the share of the files of the scan, or of
/// the procedures that have a family in the summary.
fn distributions(report: &Report, messages: &Messages) -> Vec<Distribution> {
    let summary = &report.summary;
    let files = report.scan.files_total;
    let procedures = (summary.by_family.values()).fold(0, |sum: u32, n| sum.saturating_add(*n));
    vec![
        distribution(
            messages.text("html-by-status"),
            &summary.by_status,
            files,
            |status| messages.status(*status),
        ),
        distribution(
            messages.text("html-by-complexity"),
            &summary.by_complexity,
            files,
            |level| messages.level(*level),
        ),
        distribution(
            messages.text("html-by-severity"),
            &summary.by_severity,
            files,
            |severity| messages.severity(*severity),
        ),
        distribution(
            messages.text("html-by-family"),
            &summary.by_family,
            procedures,
            |family| messages.family(family),
        ),
    ]
}

/// A row for each key of `counts`, in the order of the keys, named by `word`.
fn distribution<K>(
    title: String,
    counts: &BTreeMap<K, u32>,
    max: u32,
    word: impl Fn(&K) -> String,
) -> Distribution {
    let rows = (counts.iter())
        .map(|(key, value)| Count {
            label: word(key),
            value: *value,
        })
        .collect();
    Distribution { title, rows, max }
}

/// A view of each file of `report`, in the order of the report.
fn file_views<'a>(report: &'a Report, messages: &Messages) -> Vec<FileView<'a>> {
    (report.files.iter())
        .map(|file| FileView {
            file,
            container: messages.container(file.container),
            chips: chips(file, messages),
            references: (file.vba_project.as_ref())
                .and_then(|project| project.references.as_deref())
                .unwrap_or_default(),
            errors: file.errors.as_deref().unwrap_or_default(),
            notes: file.notes.as_deref().unwrap_or_default(),
            traits: (file.traits.iter().flatten())
                .map(|id| messages.trait_label(id))
                .collect(),
            families: family_views(file.families.as_deref().unwrap_or_default(), messages),
            signals: signal_views(file.signals.as_deref().unwrap_or_default(), messages),
        })
        .collect()
}

/// The status of a file, then its complexity level, highest severity and
/// primary family when it has them.
fn chips(file: &FileReport, messages: &Messages) -> Vec<Chip> {
    let hint = |id| Some(messages.text(id));
    let mut chips = vec![Chip::new(
        hint("html-status"),
        "status",
        &file.status,
        messages.status(file.status),
    )];
    if let Some(level) = file.complexity_level {
        chips.push(Chip::new(
            hint("html-complexity"),
            "level",
            &level,
            messages.level(level),
        ));
    }
    if let Some(severity) = file.max_severity {
        chips.push(Chip::new(
            hint("html-severity"),
            "severity",
            &severity,
            messages.severity(severity),
        ));
    }
    if let Some(family) = &file.primary_family {
        chips.push(Chip {
            hint: hint("html-family"),
            label: messages.family(family),
            class: "chip".to_owned(),
        });
    }
    chips
}

/// A view of each of `scores`, in the order of the report.
fn family_views<'a>(scores: &'a [FamilyScore], messages: &Messages) -> Vec<FamilyView<'a>> {
    (scores.iter())
        .map(|score| FamilyView {
            label: messages.family(&score.family),
            score: messages.number(score.score),
            evidence: score.evidence.as_deref().unwrap_or_default(),
        })
        .collect()
}

/// A view of each of `signals`, in the order of the report.
fn signal_views<'a>(signals: &'a [Signal], messages: &Messages) -> Vec<SignalView<'a>> {
    (signals.iter())
        .map(|signal| SignalView {
            id: &signal.signal_id,
            label: messages.signal(&signal.signal_id),
            severity: Chip::new(
                None,
                "severity",
                &signal.severity,
                messages.severity(signal.severity),
            ),
            place: match (&signal.module, &signal.procedure) {
                (Some(module), Some(procedure)) => Some(format!("{module}.{procedure}")),
                (Some(name), None) | (None, Some(name)) => Some(name.clone()),
                (None, None) => None,
            },
            evidence: signal.evidence.as_deref().unwrap_or_default(),
        })
        .collect()
}

/// The Content-Security-Policy of the page: the one inline script and the one
/// inline style, a data URI for the empty icon, and no connection.
fn policy(style: &str, script: &str) -> String {
    format!(
        "default-src 'none'; script-src {}; style-src {}; img-src data:; connect-src 'none'; \
         base-uri 'none'; form-action 'none'",
        hash_source(script),
        hash_source(style),
    )
}

/// The hash source of a policy that allows `text` as the content of an inline
/// element: the base64 of its SHA-256.
fn hash_source(text: &str) -> String {
    format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(text.as_bytes()))
    )
}

/// `text` with LF line breaks. A browser reads CRLF and CR as LF before it
/// checks a hash, and a checkout on Windows may hold a file with CRLF; the page
/// carries, and the policy hashes, the LF form.
fn lf(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `report` as compact JSON that cannot end its `<script>` element. The
/// characters escaped here occur in JSON only inside strings, where a `\u`
/// escape means the same character.
fn data_block(report: &Report) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(report)?;
    let mut block = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => block.push_str("\\u003c"),
            '>' => block.push_str("\\u003e"),
            '&' => block.push_str("\\u0026"),
            '\u{2028}' => block.push_str("\\u2028"),
            '\u{2029}' => block.push_str("\\u2029"),
            _ => block.push(c),
        }
    }
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_source_is_the_base64_of_the_sha256_of_the_text() {
        // The SHA-256 of "abc" is the test vector of FIPS 180-2, appendix B.1.
        assert_eq!(
            hash_source("abc"),
            "'sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0='"
        );
    }

    #[test]
    fn a_line_break_is_a_line_feed() {
        assert_eq!(lf("a\r\nb\rc\n"), "a\nb\nc\n");
        assert!(matches!(lf("a\nb"), Cow::Borrowed(_)));
    }
}
