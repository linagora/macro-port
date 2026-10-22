// SPDX-License-Identifier: Apache-2.0
//! Tests of the matchers, through the evaluator.

use crate::engine::testing::{line, members, project_of};
use crate::engine::{CodeLine, Evaluator, Place};
use crate::rules::{Matcher, TextMode};

/// The lines of the hits of `matcher` in a procedure whose body is `body`.
fn lines_hit(body: &[CodeLine], matcher: &Matcher) -> Vec<u32> {
    let project = project_of(body);
    let hits = Evaluator::new(&project).procedure_hits(0, 0, matcher, TextMode::Code);
    hits.iter()
        .filter_map(|hit| hit.place)
        .map(|place| place.line)
        .collect()
}

#[test]
fn member_hits_each_use_of_a_name() {
    let body = [
        line(2, "Selection.Interior.Color = 1")
            .member("Selection")
            .member("Interior")
            .member("Color"),
        line(3, "Union(a.Interior, b.Interior)")
            .member("Union")
            .member("Interior")
            .member("Interior"),
        line(4, "Range(\"A1\").Select")
            .member("Range")
            .member("Select"),
    ];
    assert_eq!(lines_hit(&body, &members(&["Interior"])), [2, 3, 3]);
    assert_eq!(lines_hit(&body, &members(&["Select", "Color"])), [2, 4]);
}

#[test]
fn member_ignores_case_and_the_type_character() {
    let body = [line(2, "x = Cells(1).VALUE$")
        .member("Cells")
        .member("VALUE$")];
    assert_eq!(lines_hit(&body, &members(&["value"])), [2]);
    assert_eq!(lines_hit(&body, &members(&["Value$"])), [2]);
}

#[test]
fn member_never_matches_a_named_argument() {
    let body = [
        line(2, "x = Selection.Before")
            .member("Selection")
            .member("Before"),
        line(3, "ws.Copy Before:=ws2")
            .member("ws")
            .member("Copy")
            .named_argument("Before"),
    ];
    assert_eq!(lines_hit(&body, &members(&["Before"])), [2]);
    assert_eq!(lines_hit(&body, &members(&["Copy"])), [3]);
}

#[test]
fn a_hit_quotes_its_statement_at_the_physical_line_of_the_statement() {
    // `a = 1: b = c.Select: d = e.Select` on line 7 is three statements that
    // share the line.
    let body = [
        line(7, "a = 1"),
        line(7, "  b = c.Select  ").member("Select"),
        line(7, "d = e.Select").member("Select"),
    ];
    let project = project_of(&body);
    let matcher = members(&["Select"]);
    let hits = Evaluator::new(&project).procedure_hits(0, 0, &matcher, TextMode::Code);
    let place = Some(Place { module: 0, line: 7 });
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].place, place);
    assert_eq!(hits[1].place, place);
    assert_eq!(hits[0].snippet(), "b = c.Select");
    assert_eq!(hits[1].snippet(), "d = e.Select");
}
