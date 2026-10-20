// SPDX-License-Identifier: Apache-2.0
//! Test helpers of Macro-Port. The other crates take this one as a
//! dev-dependency only, so it never reaches a build that ships.
//!
//! [`report_problems`] validates reports offline: jsonschema is built without
//! its HTTP and file resolvers, and the report schema only refers to itself.
//! [`corpus`] reads the test corpus.

#![forbid(unsafe_code)]

pub mod corpus;

use std::sync::LazyLock;

use jsonschema::Validator;
use serde_json::Value;

const REPORT_SCHEMA: &str = include_str!("../../../schema/report.schema.json");

static REPORT_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| {
    let schema: Value = serde_json::from_str(REPORT_SCHEMA).expect("the report schema is JSON");
    jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .expect("the report schema is a valid JSON Schema")
});

/// The problems of the JSON text `report` against
/// `schema/report.schema.json`, with its `format` keywords asserted: one line
/// per problem, the validator's message then the JSON Pointer of the value at
/// fault. Empty when the report is valid.
pub fn report_problems(report: &str) -> Vec<String> {
    let report: Value = match serde_json::from_str(report) {
        Ok(report) => report,
        Err(error) => return vec![format!("not JSON: {error}")],
    };
    REPORT_VALIDATOR
        .iter_errors(&report)
        .map(|error| match error.instance_path().as_str() {
            "" => error.to_string(),
            at => format!("{error} (at {at})"),
        })
        .collect()
}
