// SPDX-License-Identifier: Apache-2.0
//! The YAML dialect of the rules files (ADR-0002): YAML 1.2 booleans, no
//! merge keys and no custom tags. Duplicate keys stay rejected and the parse
//! budget stays on, as serde-saphyr does by default.

use serde::de::DeserializeOwned;
use serde_saphyr::options::MergeKeyPolicy;

/// Parses a rules file into an untyped document, for schema validation.
///
/// # Errors
///
/// The YAML error on one line, with its line and column.
pub fn parse_value(text: &str) -> Result<serde_json::Value, String> {
    from_str(text)
}

/// Parses a rules file into `T`, with the same error format as [`parse_value`].
pub(super) fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_saphyr::from_str_with_options(text, options()).map_err(|error| {
        error
            .without_snippet()
            .to_string()
            .replace(", set DuplicateKeyPolicy in Options if acceptable", "")
    })
}

fn options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
        merge_keys: MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
    }
}
