// SPDX-License-Identifier: Apache-2.0
//! Renderers: a report in, the text of an output file out.
//!
//! Every string of a report that comes from an analysed file (paths, module
//! and procedure names, snippets, references, error messages) is untrusted.
//! Each renderer protects it for its own format (SPEC section 8).

pub mod csv;

use serde::Serialize;
use serde_json::Value;

/// The name that the JSON report gives to `value`, a unit variant of the
/// report model such as a status or a severity.
///
/// The names are those of the schema: read from the model, not repeated here.
/// Any other value gives its JSON, so that a wrong use shows in the output.
pub(crate) fn wire_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(name)) => name,
        Ok(other) => other.to_string(),
        Err(error) => error.to_string(),
    }
}
