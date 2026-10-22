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
//! That block and the page's own stylesheet, a file of this crate, are the only
//! values that escape no character. Each is a [`Safe`] field of `Page`, so the
//! template needs no `safe` filter, and a test counts the uses of [`Safe`].
//!
//! A `<meta>` policy allows the one inline `<style>`, by its SHA-256 hash, and
//! data URIs for images, and nothing else, so a script that got in would not
//! run. It is the first element after the character set, and it also forbids
//! `<base>` and forms.

use std::borrow::Cow;
use std::collections::BTreeMap;

use askama::Template;
use askama::filters::Safe;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use macroport_core::report::Report;
use sha2::{Digest, Sha256};

use crate::i18n::Messages;
use crate::wire::wire_name;

/// The page's stylesheet, in a file so that editors know what it is. Its hash
/// is in the policy of the page.
const STYLE: &str = include_str!("../../templates/report.css");

/// The HTML report of `report`, in the language of `messages`.
///
/// # Errors
///
/// When the template fails or the report cannot be written as JSON, neither of
/// which this model does.
pub fn render(report: &Report, messages: &Messages) -> Result<String, askama::Error> {
    let style = lf(STYLE);
    Page {
        messages,
        report,
        lang: wire_name(&messages.lang()),
        policy: policy(&style),
        style: Safe(&style),
        data: Safe(data_block(report).map_err(askama::Error::custom)?),
        roots: report.scan.roots.as_deref().unwrap_or_default(),
        counts: counts(report, messages),
        distributions: distributions(report, messages),
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
    /// The page's stylesheet, a file of this crate: not escaped.
    style: Safe<&'a str>,
    /// The report as JSON that [`data_block`] made safe to put in a `<script>`
    /// element: not escaped for HTML.
    data: Safe<String>,
    /// The paths that were scanned, none when the report does not say.
    roots: &'a [String],
    counts: Vec<Count>,
    distributions: Vec<Distribution>,
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

/// The files by status, by complexity level and by highest severity. A bar
/// shows the share of the files of the scan.
fn distributions(report: &Report, messages: &Messages) -> Vec<Distribution> {
    let summary = &report.summary;
    let files = report.scan.files_total;
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

/// The Content-Security-Policy of the page: the one inline style, no script,
/// and a data URI for the empty icon.
fn policy(style: &str) -> String {
    format!(
        "default-src 'none'; style-src {}; img-src data:; base-uri 'none'; form-action 'none'",
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
