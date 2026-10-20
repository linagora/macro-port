// SPDX-License-Identifier: Apache-2.0
//! The report model.

use macroport_core::report::{
    Container, Lang, Level, ProcedureKind, Report, SCHEMA_VERSION, Status, Visibility,
};
use macroport_core::rules::{ModuleType, Severity};
use macroport_testkit::report_problems;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const EXAMPLE: &str = include_str!("../../../schema/examples/report.example.json");
const REPORT_SCHEMA: &str = include_str!("../../../schema/report.schema.json");

/// The example report, its summary emptied, then given one count per key of
/// `statuses` and `families`, inserted in their order.
fn example_with_counts(statuses: &[Status], families: &[&str]) -> serde_json::Result<Report> {
    let mut report: Report = serde_json::from_str(EXAMPLE)?;
    let summary = &mut report.summary;
    summary.by_status.clear();
    summary.by_family.clear();
    summary.by_complexity.clear();
    summary.by_severity.clear();
    summary.totals = None;
    for status in statuses {
        summary.by_status.insert(*status, 1);
    }
    for family in families {
        summary.by_family.insert((*family).to_owned(), 1);
    }
    Ok(report)
}

/// Reads a value into a model type and writes it back.
type ReadAndWrite = fn(&Value) -> serde_json::Result<Value>;

/// Reads `value` into a `T` and writes it back.
fn read_and_write<T: Serialize + DeserializeOwned>(value: &Value) -> serde_json::Result<Value> {
    serde_json::to_value(serde_json::from_value::<T>(value.clone())?)
}

#[test]
fn the_model_writes_the_example_report_as_shipped() {
    // The model denies unknown keys, so it reads every key of the example.
    let report: Report = serde_json::from_str(EXAMPLE).expect("the model reads the example");

    let json = report.to_json().expect("the report serialises");

    assert_eq!(json, EXAMPLE);
    assert_eq!(report_problems(&json), Vec::<String>::new());
}

#[test]
fn equal_reports_serialise_to_the_same_bytes() {
    let one = example_with_counts(
        &[Status::Error, Status::Ok, Status::NoMacros],
        &["unclassified", "09-events", "01-formatting"],
    )
    .expect("the model reads the example");
    let other = example_with_counts(
        &[Status::NoMacros, Status::Error, Status::Ok],
        &["01-formatting", "unclassified", "09-events"],
    )
    .expect("the model reads the example");
    assert_eq!(one, other);

    let json = one.to_json().expect("the report serialises");

    assert_eq!(other.to_json().expect("the report serialises"), json);
    assert_eq!(report_problems(&json), Vec::<String>::new());
    // Map keys come out in one order, whatever the order they went in: that
    // of the variants for statuses, alphabetical for families.
    let (_, summary) = json
        .split_once("\n  \"summary\": ")
        .expect("the report has a summary");
    assert_eq!(
        summary,
        r#"{
    "by_status": {
      "ok": 1,
      "no_macros": 1,
      "error": 1
    },
    "by_family": {
      "01-formatting": 1,
      "09-events": 1,
      "unclassified": 1
    },
    "by_complexity": {},
    "by_severity": {}
  }
}
"#
    );
}

#[test]
fn every_enum_value_of_the_schema_reads_into_the_model_and_back() {
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("the report schema is JSON");
    let enums: [(&str, ReadAndWrite); 8] = [
        ("/properties/lang/enum", read_and_write::<Lang>),
        ("/$defs/severity/enum", read_and_write::<Severity>),
        ("/$defs/level/enum", read_and_write::<Level>),
        (
            "/$defs/procedure/properties/kind/enum",
            read_and_write::<ProcedureKind>,
        ),
        (
            "/$defs/procedure/properties/visibility/enum",
            read_and_write::<Visibility>,
        ),
        (
            "/$defs/module/properties/type/enum",
            read_and_write::<ModuleType>,
        ),
        (
            "/$defs/file/properties/container/enum",
            read_and_write::<Container>,
        ),
        (
            "/$defs/file/properties/status/enum",
            read_and_write::<Status>,
        ),
    ];

    for (pointer, read_and_write) in enums {
        let values = schema
            .pointer(pointer)
            .and_then(Value::as_array)
            .expect("the schema has the enum");
        for value in values {
            assert_eq!(
                read_and_write(value).expect("the model reads the value"),
                *value,
                "{pointer}"
            );
        }
    }
}

#[test]
fn the_model_writes_the_schema_version_of_the_report_schema() {
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("the report schema is JSON");

    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        SCHEMA_VERSION
    );
}

#[test]
fn module_types_are_written_under_their_rules_names() {
    for module_type in ModuleType::ALL {
        assert_eq!(
            serde_json::to_value(module_type).expect("the module type serialises"),
            module_type.name()
        );
    }
}
