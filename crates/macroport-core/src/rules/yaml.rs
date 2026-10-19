// SPDX-License-Identifier: Apache-2.0
//! The YAML dialect of the rules files (ADR-0002): YAML 1.2 booleans, no
//! merge keys and no custom tags. Duplicate keys stay rejected and the parse
//! budget stays on, as serde-saphyr does by default.

use std::fmt;

use serde::Deserializer;
use serde::de::{self, DeserializeOwned, Unexpected, Visitor};
use serde_saphyr::options::MergeKeyPolicy;

/// Advice serde-saphyr gives its own callers, meaningless to a rules author.
const LIBRARY_HINTS: [&str; 2] = [
    ", set DuplicateKeyPolicy in Options if acceptable",
    "; use from_str_multiple or from_str_multiple_with_options",
];

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
        LIBRARY_HINTS
            .iter()
            .fold(error.without_snippet().to_string(), |message, hint| {
                message.replace(hint, "")
            })
    })
}

fn options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
        merge_keys: MergeKeyPolicy::Error,
        reject_unsupported_tags: true,
    }
}

/// A whole number however YAML spells it (`3`, `3.0`, `1e1`, `0x10`): what
/// the schemas' `integer` accepts, so that a file passing its schema loads
/// with the same values. Quoted numbers stay strings, as in the schemas.
pub(super) fn whole_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    deserializer.deserialize_any(WholeNumber)
}

/// [`whole_number`] for a key that may be left out, with `#[serde(default)]`.
pub(super) fn optional_whole_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    whole_number(deserializer).map(Some)
}

struct WholeNumber;

impl Visitor<'_> for WholeNumber {
    type Value = u32;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a whole number")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
        u32::try_from(value).map_err(|_| E::invalid_value(Unexpected::Unsigned(value), &self))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<u32, E> {
        u32::try_from(value).map_err(|_| E::invalid_value(Unexpected::Signed(value), &self))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<u32, E> {
        if value.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&value) {
            // Whole and in range: the cast is exact.
            Ok(value as u32)
        } else {
            Err(E::invalid_value(Unexpected::Float(value), &self))
        }
    }
}
