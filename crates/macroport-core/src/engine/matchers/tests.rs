// SPDX-License-Identifier: Apache-2.0
//! Tests of the `member`, `chain`, `call` and `progid` matchers, through the
//! evaluator. The other matchers have a test module of their own.

use crate::engine::testing::{
    calls, chains, line, lines_hit, lines_of, members, progids, project_of,
};
use crate::engine::{Evaluator, Place};
use crate::rules::{Families, Family, FamilySignal, Label, Scope, Scoring, TextMode};

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

#[test]
fn a_matcher_without_a_name_matches_nothing() {
    // Nor does a use without a name, which no parser should give.
    let body = [line(2, "a.b c")
        .member("b")
        .member("c")
        .member("")
        .member("$")
        .chain("a.b")
        .chain("")
        .call("c")
        .call("")];
    let matchers = [
        members(&[]),
        chains(&[]),
        calls(&[]),
        members(&[""]),
        members(&["$"]),
        chains(&[""]),
        calls(&[""]),
    ];
    for matcher in matchers {
        assert!(lines_hit(&body, &matcher).is_empty());
    }
    // The same lines do give hits for the names that they hold.
    assert_eq!(lines_hit(&body, &members(&["b", "c"])), [2, 2]);
    assert_eq!(lines_hit(&body, &chains(&["a.b"])), [2]);
    assert_eq!(lines_hit(&body, &calls(&["c"])), [2]);
}

#[test]
fn chain_matches_a_pattern_wherever_it_is_in_a_chain() {
    let body = [
        line(2, "Workbooks.Open f").chain("Workbooks.Open"),
        line(3, "v = Application.Caller.Address").chain("Application.Caller.Address"),
        line(4, "Worksheets(1).Range(a).Interior.Color = 1")
            .chain("Worksheets.Range.Interior.Color"),
        // The example of SPEC section 7: a pattern at the end of the chain.
        line(5, "Worksheets(1).Range(\"A1\").Interior").chain("Worksheets.Range.Interior"),
        line(6, "Workbooks.Close").chain("Workbooks.Close"),
    ];
    let matcher = chains(&["Workbooks.Open", "Application.Caller", "Range.Interior"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 3, 4, 5]);
}

#[test]
fn chain_compares_whole_elements_in_order() {
    let body = [
        line(2, "Workbooks.OpenText f").chain("Workbooks.OpenText"),
        line(3, "ThisWorkbooks.Open f").chain("ThisWorkbooks.Open"),
        line(4, "Workbooks.Item.Open").chain("Workbooks.Item.Open"),
        line(5, "Open.Workbooks").chain("Open.Workbooks"),
        line(6, "Workbooks").chain("Workbooks"),
    ];
    assert!(lines_hit(&body, &chains(&["Workbooks.Open"])).is_empty());
}

#[test]
fn chain_ignores_case_and_type_characters() {
    let body = [line(2, "x = f.VLOOKUP$(a)").chain("worksheetfunction.VLOOKUP$")];
    let matcher = chains(&["WorksheetFunction.VLookup"]);
    assert_eq!(lines_hit(&body, &matcher), [2]);
}

#[test]
fn chain_gives_one_hit_per_chain_whatever_the_patterns() {
    let body = [line(2, "f Workbooks.Open, Workbooks.Open")
        .chain("Workbooks.Open")
        .chain("Workbooks.Open")];
    let matcher = chains(&["Workbooks.Open", "Open"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 2]);
}

#[test]
fn call_matches_a_name_invoked_on_its_own_and_not_after_a_dot() {
    let body = [
        line(2, "s = Replace(a, b, c)").call("Replace"),
        line(3, "Cells.Replace What:=a").call_after_dot("Replace"),
    ];
    assert_eq!(lines_hit(&body, &calls(&["Replace"])), [2]);
}

#[test]
fn call_ignores_case_and_type_characters() {
    let body = [
        line(2, "s = Chr$(65)").call("Chr$"),
        line(3, "t = MID(s, 1)").call("MID"),
    ];
    assert_eq!(lines_hit(&body, &calls(&["chr", "Mid$"])), [2, 3]);
}

#[test]
fn progid_ignores_case() {
    let body = [line(2, "CreateObject(\"outlook.application\")").progid("outlook.application")];
    assert_eq!(lines_hit(&body, &progids(&["Outlook.Application"])), [2]);
    assert!(lines_hit(&body, &progids(&["Word.Application"])).is_empty());
}

#[test]
fn progid_patterns_are_globs() {
    let body = [
        line(2, "CreateObject(a)").progid("MSXML2.XMLHTTP"),
        line(3, "CreateObject(b)").progid("MSXML2.XMLHTTP.6.0"),
        line(4, "CreateObject(c)").progid("MSXML2.DOMDocument"),
        line(5, "CreateObject(d)").progid("Word.Document.8"),
        line(6, "CreateObject(e)").progid("Word.Document.12"),
    ];
    assert_eq!(lines_hit(&body, &progids(&["MSXML2.XMLHTTP*"])), [2, 3]);
    assert_eq!(lines_hit(&body, &progids(&["Word.Document.?"])), [5]);
}

#[test]
fn progid_gives_one_hit_per_progid_whatever_the_patterns() {
    let body = [
        line(2, "CreateObject(a)").progid("Outlook.Application"),
        line(3, "Use CreateObject(b), CreateObject(d)")
            .progid("Outlook.Application")
            .progid("Word.Application"),
    ];
    let matcher = progids(&["Outlook.*", "*.Application", "Outlook.Application"]);
    assert_eq!(lines_hit(&body, &matcher), [2, 3, 3]);
}

#[test]
fn progid_ignores_what_is_not_shaped_like_a_progid() {
    let body = [
        line(2, "GetObject(a)").progid("c:\\vb4\\MYTEST.XLS"),
        line(3, "GetObject(b)").progid(""),
        line(4, "CreateObject(c)").progid("Word."),
        line(5, "CreateObject(d)").progid("1Word.Application"),
        line(6, "CreateObject(e)").progid("Word Application"),
        line(7, "CreateObject(f)").progid("Word"),
        line(8, "CreateObject(g)").progid("Word.Application.16"),
        line(9, "CreateObject(h)").progid("Contoso_Widget.Item2"),
    ];
    // Not even a glob that fits the text reaches such a string.
    assert_eq!(lines_hit(&body, &progids(&["*"])), [8, 9]);
    assert_eq!(lines_hit(&body, &progids(&["c:*", "*", "Word*"])), [8, 9]);
    assert_eq!(lines_hit(&body, &progids(&["Word*"])), [8]);
}

#[test]
fn progid_star_matches_any_progid_for_the_security_signals() {
    let body = [
        line(2, "CreateObject(a)").progid("Scripting.Dictionary"),
        line(3, "CreateObject(b)").progid("Contoso.Widget"),
        line(4, "x = 1"),
    ];
    assert_eq!(lines_hit(&body, &progids(&["*"])), [2, 3]);
}

/// Family rules with a `progid` signal for each list of patterns.
fn families_with(signals: &[&[&str]]) -> Families {
    let signals = signals
        .iter()
        .enumerate()
        .map(|(index, patterns)| FamilySignal {
            id: format!("signal.{index}"),
            matcher: progids(patterns),
            weight: 1,
            max_hits_per_procedure: 3,
            scope: Scope::Procedure,
            text: TextMode::Code,
            label: None,
        })
        .collect();
    Families {
        version: String::new(),
        scoring: Scoring {
            min_score: 3.0,
            secondary_ratio: 0.6,
            default_max_hits_per_procedure: 3,
        },
        families: vec![Family {
            id: "family".to_owned(),
            corpus_slug: "family".to_owned(),
            label: Label {
                en: String::new(),
                fr: String::new(),
            },
            signals,
        }],
    }
}

#[test]
fn progid_star_of_the_families_skips_what_another_signal_matches() {
    // The last signal is the catch-all: it claims nothing.
    let families = families_with(&[
        &["Scripting.Dictionary"],
        &["MSXML2.XMLHTTP*", "WinHttp.*"],
        &["*"],
    ]);
    let body = [
        line(2, "CreateObject(a)").progid("Scripting.Dictionary"),
        line(3, "CreateObject(b)").progid("MSXML2.XMLHTTP.6.0"),
        line(4, "CreateObject(c)").progid("Contoso.Widget"),
        line(5, "CreateObject(d)").progid("SCRIPTING.DICTIONARY"),
        line(6, "CreateObject(e)").progid("WinHttp.WinHttpRequest.5.1"),
    ];
    let project = project_of(&body);
    let evaluator = Evaluator::for_families(&project, &families);
    let hits = evaluator.procedure_hits(0, 0, &progids(&["*"]), TextMode::Code);
    assert_eq!(lines_of(&hits), [4]);
    // A glob that is not only `*` is an ordinary glob.
    let hits = evaluator.procedure_hits(0, 0, &progids(&["Contoso.*"]), TextMode::Code);
    assert_eq!(lines_of(&hits), [4]);
    let hits = evaluator.procedure_hits(0, 0, &progids(&["Scripting.*"]), TextMode::Code);
    assert_eq!(lines_of(&hits), [2, 5]);
    // Without the family rules, `*` is any ProgID.
    let hits = Evaluator::new(&project).procedure_hits(0, 0, &progids(&["*"]), TextMode::Code);
    assert_eq!(lines_of(&hits), [2, 3, 4, 5, 6]);
}

#[test]
fn progid_star_of_the_families_matches_all_when_no_other_signal_has_a_progid() {
    let families = families_with(&[&["*"]]);
    let body = [line(2, "CreateObject(a)").progid("Scripting.Dictionary")];
    let project = project_of(&body);
    let evaluator = Evaluator::for_families(&project, &families);
    let hits = evaluator.procedure_hits(0, 0, &progids(&["*"]), TextMode::Code);
    assert_eq!(lines_of(&hits), [2]);
}
