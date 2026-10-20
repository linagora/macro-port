// SPDX-License-Identifier: Apache-2.0
//! The matchers of SPEC section 7, each over the lines of a few units.
//!
//! A matcher pushes one hit per match, in the order it finds them. The
//! evaluator sorts the hits.

use super::eval::Unit;
use super::hit::{Hit, Place};
use super::input::CodeLine;
use super::names::{fold_all, fold_name};

/// Every line of the units, with the index of its module.
fn lines<'a, 'p>(units: &'a [Unit<'p>]) -> impl Iterator<Item = (usize, &'p CodeLine)> + 'a {
    units
        .iter()
        .flat_map(|unit| unit.lines.iter().map(move |line| (unit.module, line)))
}

/// A hit that quotes the statement of `line`.
fn on_line<'p>(module: usize, line: &'p CodeLine) -> Hit<'p> {
    let place = Place {
        module,
        line: line.line,
    };
    Hit::new(Some(place), &line.raw)
}

/// `member`: an identifier used as a member or as a standalone object name,
/// never a named argument. One hit per use.
pub(super) fn member<'p>(units: &[Unit<'p>], names: &[String], hits: &mut Vec<Hit<'p>>) {
    let names = fold_all(names);
    for (module, line) in lines(units) {
        for member in line.members.iter().filter(|member| !member.named_argument) {
            if names.contains(&fold_name(&member.name)) {
                hits.push(on_line(module, line));
            }
        }
    }
}

#[cfg(test)]
mod tests;
