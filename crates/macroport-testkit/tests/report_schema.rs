// SPDX-License-Identifier: Apache-2.0
//! Validating reports against `schema/report.schema.json`.

use macroport_testkit::report_problems;
use serde_json::{Value, json};

const EXAMPLE: &str = include_str!("../../../schema/examples/report.example.json");

/// The example report with the value at `pointer` replaced by `value`.
fn broken_example(pointer: &str, value: Value) -> String {
    let mut report: Value = serde_json::from_str(EXAMPLE).expect("the example is JSON");
    *report
        .pointer_mut(pointer)
        .expect("the example has the value") = value;
    report.to_string()
}

#[test]
fn example_report_is_valid() {
    assert_eq!(report_problems(EXAMPLE), Vec::<String>::new());
}

#[test]
fn formats_are_asserted() {
    let report = broken_example("/generated_at", json!("yesterday"));

    assert_eq!(
        report_problems(&report),
        [r#""yesterday" is not a "date-time" (at /generated_at)"#]
    );
}

#[test]
fn problems_give_the_path_of_the_value_at_fault() {
    let report = broken_example(
        "/files/0/vba_project/modules/0/procedures/0/kind",
        json!("macro"),
    );

    assert_eq!(
        report_problems(&report),
        [
            r#""macro" is not one of "sub", "function" or 3 other candidates (at /files/0/vba_project/modules/0/procedures/0/kind)"#
        ]
    );
}

#[test]
fn problems_of_the_whole_report_have_no_path() {
    let mut report: Value = serde_json::from_str(EXAMPLE).expect("the example is JSON");
    report
        .as_object_mut()
        .expect("the example is an object")
        .remove("summary");

    assert_eq!(
        report_problems(&report.to_string()),
        [r#""summary" is a required property"#]
    );
}

#[test]
fn text_that_is_not_json_is_a_problem() {
    assert_eq!(
        report_problems("{ \"schema_version\": "),
        ["not JSON: EOF while parsing a value at line 1 column 20"]
    );
}
