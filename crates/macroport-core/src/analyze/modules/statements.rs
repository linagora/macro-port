// SPDX-License-Identifier: Apache-2.0
//! The statements of a module, read once for what they show of the file (SPEC
//! M2 items 3 and 4).
//!
//! The parser and the metrics read the lines of a module for their structure.
//! The facts of a statement ([`Facts`]) are what the other stages read: the
//! names it invokes, the chains it follows, the strings it holds and the
//! ProgIDs it creates. They hold their own copies of those names and strings,
//! so none is kept: [`each_statement`] hands them to a visitor one at a time,
//! and what a stage keeps of them is a count, a flag or a small set.
//!
//! The statements are read in *runs*: the lines of one procedure, or the lines
//! outside the procedures that lie together (before the first, between two,
//! after the last). Each run has a reader of its own, so that a `With` block
//! that a run leaves open (a procedure with no `End`) closes with it and gives
//! its object to no statement of another. The constants of the module are
//! declared before any run is read, since a procedure may use one that the
//! module declares below it.

use std::collections::BTreeSet;
use std::ops::Range;

use super::Analysis;
use crate::vba::{ConstTable, Facts, LogicalLine, Procedure, StatementReader, is_external_progid};

/// The most distinct ProgIDs that a file keeps. Real code names a handful, and
/// a hostile file can name as many as its text holds calls; past the limit a
/// ProgID that is new is not counted.
const MAX_PROGIDS: usize = 10_000;

/// Where a statement lies in its module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Place {
    /// Outside every procedure: a declaration, or code that no procedure holds.
    Outside,
    /// In the procedure at this position among those of the module.
    Procedure(usize),
}

/// Calls `visit` for every logical line of the module `analysis` holds, in
/// source order, with where it lies, its position among the logical lines of the
/// module, the line itself and the facts of its statement.
pub(super) fn each_statement(
    analysis: &Analysis,
    mut visit: impl FnMut(Place, usize, &LogicalLine, &Facts),
) {
    let logical = &analysis.lines.logical;
    let runs = runs(logical.len(), &analysis.module.procedures);
    let mut consts = ConstTable::default();
    for (_, range) in runs.iter().filter(|(place, _)| *place == Place::Outside) {
        for line in slice(logical, range) {
            consts.declare(line);
        }
    }
    for (place, range) in runs {
        let mut reader = StatementReader::new(&consts);
        for (position, line) in (range.start..).zip(slice(logical, &range)) {
            let facts = reader.read(line);
            visit(place, position, line, &facts);
        }
    }
}

/// The runs of a module of `count` logical lines, in order: the lines of each
/// procedure and the lines outside the procedures, none empty. The parser keeps
/// its procedures in order and apart, but a hostile module is no reason to count
/// on it: a run is cut to what comes after the one before.
fn runs(count: usize, procedures: &[Procedure]) -> Vec<(Place, Range<usize>)> {
    let mut runs = Vec::new();
    let mut next = 0;
    for (index, procedure) in procedures.iter().enumerate() {
        let start = procedure.lines.start.max(next);
        let end = procedure.lines.end.min(count);
        if start >= end {
            continue;
        }
        if start > next {
            runs.push((Place::Outside, next..start));
        }
        runs.push((Place::Procedure(index), start..end));
        next = end;
    }
    if count > next {
        runs.push((Place::Outside, next..count));
    }
    runs
}

/// The lines of `range`. `runs` keeps its ranges within the lines, so the range
/// is never out of them; `get` is how the lints let a slice be taken.
fn slice<'a>(lines: &'a [LogicalLine], range: &Range<usize>) -> &'a [LogicalLine] {
    lines.get(range.clone()).unwrap_or_default()
}

/// The classes that count as external dependencies among those that the
/// statements of a file pass to `CreateObject` and `GetObject`: each once
/// whatever its letter case, in lower case, and no more than [`MAX_PROGIDS`].
#[derive(Debug, Default)]
pub(super) struct ExternalProgids(BTreeSet<String>);

impl ExternalProgids {
    /// Adds the classes that the statements of `analysis` pass to
    /// `CreateObject` and `GetObject`.
    pub(super) fn read(&mut self, analysis: &Analysis) {
        each_statement(analysis, |_, _, _, facts| {
            for progid in &facts.progids {
                self.add(progid);
            }
        });
    }

    /// Adds `progid`, a class that a statement passes to `CreateObject` or
    /// `GetObject`, if it counts as an external dependency and the set has room
    /// for it: it is full at [`MAX_PROGIDS`], whereupon a new class is not
    /// counted.
    fn add(&mut self, progid: &str) {
        if self.0.len() < MAX_PROGIDS && is_external_progid(progid) {
            self.0.insert(progid.to_ascii_lowercase());
        }
    }

    /// The classes, in lower case and in order.
    pub(super) fn iter(&self) -> impl Iterator<Item = &String> {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{ProcedureKind, Visibility};

    fn analysis(text: &str) -> Analysis {
        Analysis::of(text).expect("a short module")
    }

    /// A procedure whose logical lines are `lines`.
    fn procedure(lines: Range<usize>) -> Procedure {
        Procedure {
            name: "P".to_owned(),
            kind: ProcedureKind::Sub,
            visibility: Visibility::Default,
            is_static: false,
            parameter_count: 0,
            start_line: 1,
            end_line: 1,
            lines,
            recoveries: Vec::new(),
        }
    }

    /// The places and positions of the statements of `text`, in the order they
    /// are visited.
    fn visited(text: &str) -> Vec<(Place, usize)> {
        let mut found = Vec::new();
        each_statement(&analysis(text), |place, position, _, _| {
            found.push((place, position));
        });
        found
    }

    #[test]
    fn the_runs_are_the_procedures_and_the_lines_between_them() {
        let procedures = [procedure(1..3), procedure(3..4), procedure(6..7)];

        assert_eq!(
            runs(9, &procedures),
            [
                (Place::Outside, 0..1),
                (Place::Procedure(0), 1..3),
                (Place::Procedure(1), 3..4),
                (Place::Outside, 4..6),
                (Place::Procedure(2), 6..7),
                (Place::Outside, 7..9),
            ]
        );
        assert_eq!(runs(2, &[]), [(Place::Outside, 0..2)]);
        assert_eq!(runs(0, &[]), []);
    }

    #[test]
    fn a_procedure_that_overlaps_or_leaves_the_module_is_cut_to_what_is_left() {
        // The second starts inside the first, the third lies past the end of
        // the module, the fourth is empty and the last is wholly past it.
        let procedures = [
            procedure(0..4),
            procedure(2..6),
            procedure(5..20),
            procedure(7..7),
            procedure(30..40),
        ];

        assert_eq!(
            runs(8, &procedures),
            [
                (Place::Procedure(0), 0..4),
                (Place::Procedure(1), 4..6),
                (Place::Procedure(2), 6..8),
            ]
        );
    }

    #[test]
    fn every_logical_line_is_visited_once_in_source_order() {
        let text = "Option Explicit\nSub A()\n    x = 1\nEnd Sub\nDim y\nSub B()\nEnd Sub\n";

        assert_eq!(
            visited(text),
            [
                (Place::Outside, 0),
                (Place::Procedure(0), 1),
                (Place::Procedure(0), 2),
                (Place::Procedure(0), 3),
                (Place::Outside, 4),
                (Place::Procedure(1), 5),
                (Place::Procedure(1), 6),
            ]
        );
        assert_eq!(visited(""), []);
    }

    /// The ProgIDs that the statements of `text` give, as the file keeps them.
    fn progids(text: &str) -> Vec<String> {
        let mut found = ExternalProgids::default();
        found.read(&analysis(text));
        found.iter().cloned().collect()
    }

    #[test]
    fn the_constants_of_a_module_fold_in_the_procedures_above_them() {
        let text = "Sub A()\n    Set o = CreateObject(Prog)\nEnd Sub\n\
                    Private Const Prog = \"Word.\" & \"Application\"\n";

        assert_eq!(progids(text), ["word.application"]);
    }

    #[test]
    fn a_with_block_that_a_procedure_leaves_open_ends_with_it() {
        // `A` has no end, so the parser closes it where `B` starts, and the
        // block it opened does not reach the dot of `B`.
        let analysis =
            analysis("Sub A()\n    With Range(\"A1\")\nSub B()\n    .Font = 1\nEnd Sub\n");
        let mut chains = Vec::new();

        each_statement(&analysis, |place, _, _, facts| {
            if place == Place::Procedure(1) {
                chains.extend(facts.chains.clone());
            }
        });

        assert_eq!(analysis.module.procedures.len(), 2);
        assert!(chains.is_empty(), "{chains:?}");
    }

    #[test]
    fn only_the_classes_that_count_as_dependencies_are_kept() {
        let text = "Sub A()\n    Set a = CreateObject(\"WScript.Shell\")\n\
                    \x20   Set b = CreateObject(\"wscript.SHELL\")\n\
                    \x20   Set c = CreateObject(\"Scripting.Dictionary\")\n\
                    \x20   Set d = GetObject(\"C:\\book.xlsx\")\n\
                    \x20   Set e = GetObject(, \"Word.Application\")\n\
                    \x20   Set f = CreateObject(\"htmlfile\")\nEnd Sub\n";

        // The dictionary is common, the path is no class, and `htmlfile` has no dot.
        assert_eq!(progids(text), ["word.application", "wscript.shell"]);
    }

    #[test]
    fn a_hostile_file_cannot_make_the_set_of_progids_grow_without_limit() {
        let mut known = ExternalProgids::default();

        for number in 0..MAX_PROGIDS + 5 {
            known.add(&format!("Vendor.Class{number}"));
        }
        // The set is full: a class that it has changes nothing, and a new one
        // is not counted.
        known.add("VENDOR.CLASS0");
        known.add("Other.Class");

        assert_eq!(known.0.len(), MAX_PROGIDS);
        assert!(known.0.contains("vendor.class0") && !known.0.contains("other.class"));
    }
}
