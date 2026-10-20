// SPDX-License-Identifier: Apache-2.0
//! The matchers of SPEC section 7, each over a few units. Most read the lines
//! of the units, the others the procedure and the module that a unit stands
//! for.
//!
//! A matcher pushes one hit per match, in the order it finds them. The
//! evaluator sorts the hits.

use std::collections::BTreeSet;

use regex::Regex;

use super::glob::glob_matches;
use super::hit::{Hit, Place};
use super::input::{CallSite, CodeLine, MemberUse, ProcedureInput};
use super::names::{same_name, without_type_char};
use super::unit::Unit;
use crate::rules::{ModuleType, TextMode};

mod structural;

pub(super) use structural::structural;
pub use structural::{is_auto_exec, is_event_handler};

/// The `progid` pattern that stands for any ProgID.
pub(super) const ANY_PROGID: &str = "*";

/// Every line of the units, with the index of its module.
fn lines<'a, 'p>(units: &'a [Unit<'p>]) -> impl Iterator<Item = (usize, &'p CodeLine)> + 'a {
    units
        .iter()
        .flat_map(|unit| unit.lines().iter().map(move |line| (unit.module, line)))
}

/// A hit at `line` of the module `module`, quoting `text`.
fn hit_at(module: usize, line: u32, text: &str) -> Hit<'_> {
    Hit::new(Some(Place { module, line }), text)
}

/// A hit that quotes the statement of `line`.
fn statement_hit<'p>(module: usize, line: &'p CodeLine) -> Hit<'p> {
    hit_at(module, line.line, &line.raw)
}

/// A hit about the whole of `procedure`: it cites the declaration, which it
/// quotes. A procedure that the parser gave no line is quoted by its name.
fn declaration_hit<'p>(module: usize, procedure: &'p ProcedureInput) -> Hit<'p> {
    let declaration = procedure
        .lines
        .first()
        .map_or(procedure.name.as_str(), |line| line.raw.as_str());
    hit_at(module, procedure.start_line, declaration)
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

/// Whether one of the regexes finds a match in `subject`.
fn any_match(patterns: &[Regex], subject: &str) -> bool {
    patterns.iter().any(|pattern| pattern.is_match(subject))
}

/// `procedure_name`: a regex on the name of a procedure, without the
/// type-declaration character that may end it. One hit per procedure, at its
/// declaration. The declarations of a module have no name to match.
pub(super) fn procedure_name<'p>(units: &[Unit<'p>], patterns: &[Regex], hits: &mut Vec<Hit<'p>>) {
    for unit in units {
        if let Some(procedure) = unit.procedure
            && any_match(patterns, without_type_char(&procedure.name))
        {
            hits.push(declaration_hit(unit.module, procedure));
        }
    }
}

/// `module_type`: a module of one of the types. The type belongs to the module
/// as a whole, so there is one hit per module, whatever the units that cover
/// it. Over a module or a project the hit is at the first line of the module
/// and quotes its name. Over one procedure it is at the declaration of the
/// procedure, which is in the procedure.
pub(super) fn module_type<'p>(units: &[Unit<'p>], types: &[ModuleType], hits: &mut Vec<Hit<'p>>) {
    let mut seen = BTreeSet::new();
    for unit in units {
        if !types.contains(&unit.owner.module_type) {
            continue;
        }
        // One hit per module: the first of its units speaks for the others.
        if !seen.insert(unit.module) {
            continue;
        }
        hits.push(match unit.procedure {
            Some(procedure) => declaration_hit(unit.module, procedure),
            None => hit_at(unit.module, 1, &unit.owner.name),
        });
    }
}

/// `reference`: a regex on the display name of a reference of the project. One
/// hit per reference, with no place: a reference is not in the source of a
/// module, so the caller cites the line of its choice.
pub(super) fn reference<'p>(references: &'p [String], patterns: &[Regex], hits: &mut Vec<Hit<'p>>) {
    for name in references {
        if any_match(patterns, name) {
            hits.push(Hit::new(None, name));
        }
    }
}

/// `regex`: a regex on a logical line, on its code (string literals and the
/// comment blanked) or on its raw text. One hit per line, however many
/// regexes or times it matches.
pub(super) fn regex<'p>(
    units: &[Unit<'p>],
    patterns: &[Regex],
    text: TextMode,
    hits: &mut Vec<Hit<'p>>,
) {
    for (module, line) in lines(units) {
        let subject = match text {
            TextMode::Code => &line.code,
            TextMode::Raw => &line.raw,
        };
        if any_match(patterns, subject) {
            hits.push(statement_hit(module, line));
        }
    }
}

/// `string`: a regex on the contents of a string literal, as folded by the
/// parser. One hit per literal.
pub(super) fn string<'p>(units: &[Unit<'p>], patterns: &[Regex], hits: &mut Vec<Hit<'p>>) {
    let fits = |literal: &String| any_match(patterns, literal);
    per_item(units, |line| line.strings.as_slice(), fits, hits);
}

/// `comment`: a regex on the text of the comment that ends a line. One hit per
/// comment.
pub(super) fn comment<'p>(units: &[Unit<'p>], patterns: &[Regex], hits: &mut Vec<Hit<'p>>) {
    let fits = |text: &String| any_match(patterns, text);
    per_item(units, |line| line.comment.as_slice(), fits, hits);
}

#[cfg(test)]
mod project_tests;
#[cfg(test)]
mod structural_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;
