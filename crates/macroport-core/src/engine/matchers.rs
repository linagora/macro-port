// SPDX-License-Identifier: Apache-2.0
//! The matchers of SPEC section 7, each over the lines of a few units.
//!
//! A matcher pushes one hit per match, in the order it finds them. The
//! evaluator sorts the hits.

use super::glob::glob_matches;
use super::hit::{Hit, Place};
use super::input::{CallSite, CodeLine, MemberUse};
use super::names::same_name;
use super::unit::Unit;

/// The `progid` pattern that stands for any ProgID.
pub(super) const ANY_PROGID: &str = "*";

/// Every line of the units, with the index of its module.
fn lines<'a, 'p>(units: &'a [Unit<'p>]) -> impl Iterator<Item = (usize, &'p CodeLine)> + 'a {
    units
        .iter()
        .flat_map(|unit| unit.lines.iter().map(move |line| (unit.module, line)))
}

/// A hit that quotes the statement of `line`.
fn statement_hit<'p>(module: usize, line: &'p CodeLine) -> Hit<'p> {
    let place = Place {
        module,
        line: line.line,
    };
    Hit::new(Some(place), &line.raw)
}

/// A hit for each item that `fits`, among the items of each line that `items`
/// gives. This is the shape of the matchers over what the parser found in a
/// statement.
fn per_item<'p, T: 'p>(
    units: &[Unit<'p>],
    items: impl Fn(&'p CodeLine) -> &'p [T],
    fits: impl Fn(&T) -> bool,
    hits: &mut Vec<Hit<'p>>,
) {
    for (module, line) in lines(units) {
        for item in items(line) {
            if fits(item) {
                hits.push(statement_hit(module, line));
            }
        }
    }
}

/// `member`: an identifier used as a member or as a standalone object name,
/// never a named argument. One hit per use.
pub(super) fn member<'p>(units: &[Unit<'p>], names: &[String], hits: &mut Vec<Hit<'p>>) {
    let fits = |used: &MemberUse| {
        !used.named_argument && names.iter().any(|name| same_name(name, &used.name))
    };
    per_item(units, |line| line.members.as_slice(), fits, hits);
}

/// `chain`: a chain that holds the dot-separated names of a pattern as
/// consecutive elements, so that `Range.Interior` matches
/// `Worksheets.Range.Interior.Color`. Elements compare whole: `Workbooks.Open`
/// does not match `Workbooks.OpenText`. One hit per chain.
pub(super) fn chain<'p>(units: &[Unit<'p>], patterns: &[String], hits: &mut Vec<Hit<'p>>) {
    let fits = |elements: &Vec<String>| {
        patterns
            .iter()
            .any(|pattern| contains_run(elements, pattern))
    };
    per_item(units, |line| line.chains.as_slice(), fits, hits);
}

/// Whether the dot-separated names of `pattern` are found among `elements` as
/// consecutive elements.
fn contains_run(elements: &[String], pattern: &str) -> bool {
    // `split` yields at least one name, as `windows` needs.
    let wanted = pattern.split('.').count();
    wanted > 0
        && elements.windows(wanted).any(|window| {
            window
                .iter()
                .zip(pattern.split('.'))
                .all(|(element, name)| same_name(element, name))
        })
}

/// `call`: a procedure, a function or a statement invoked by name, not
/// preceded by `.`. One hit per call.
pub(super) fn call<'p>(units: &[Unit<'p>], names: &[String], hits: &mut Vec<Hit<'p>>) {
    let fits = |invoked: &CallSite| {
        !invoked.after_dot && names.iter().any(|name| same_name(name, &invoked.name))
    };
    per_item(units, |line| line.calls.as_slice(), fits, hits);
}

/// `progid`: a ProgID passed to `CreateObject` or `GetObject` that fits a glob
/// pattern, whatever its case. The pattern `*` alone is for `catch_all` to
/// decide (SPEC section 7). One hit per ProgID, however many patterns fit. A
/// string that is not shaped like a ProgID fits no pattern.
pub(super) fn progid<'p>(
    units: &[Unit<'p>],
    patterns: &[String],
    catch_all: impl Fn(&str) -> bool,
    hits: &mut Vec<Hit<'p>>,
) {
    let fits = |progid: &String| {
        is_progid(progid)
            && patterns.iter().any(|pattern| match pattern.as_str() {
                ANY_PROGID => catch_all(progid),
                _ => glob_matches(pattern, progid),
            })
    };
    per_item(units, |line| line.progids.as_slice(), fits, hits);
}

/// Whether `text` is shaped like a ProgID: a letter, then letters, digits, `_`
/// and `.`, with a `.` that does not end it, as in `Word.Application.16`. A
/// file name or a moniker given to `GetObject` is not one.
fn is_progid(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
        && text.contains('.')
        && !text.ends_with('.')
}

#[cfg(test)]
mod tests;
