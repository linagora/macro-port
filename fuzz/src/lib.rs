// SPDX-License-Identifier: Apache-2.0
//! Checks shared by the fuzz targets.
//!
//! A target fails when the code under test panics, hangs or runs out of
//! memory, and also when its result breaks a contract the core promises, such
//! as the shape of an error.

use macroport_core::report::Status;

/// Asserts that a refusal has the shape every layer of the core promises: a
/// status that says the file failed, a lowercase dotted code
/// `<layer>.<problem>` (such as `cfb.fat_loop` or `ovba.dir.truncated`) and a
/// message. `layer` is the first segment of the code, when the caller knows it.
pub fn assert_typed_error(layer: Option<&str>, status: Status, code: &str, message: &str) {
    assert!(
        !matches!(status, Status::Ok | Status::NoMacros),
        "{code}: a refusal cannot give the status {status:?}"
    );
    let segments: Vec<&str> = code.split('.').collect();
    let lowercase = |segment: &&str| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    };
    assert!(
        segments.len() >= 2 && segments.iter().all(lowercase),
        "{code:?} is not a lowercase dotted code"
    );
    if let Some(layer) = layer {
        assert_eq!(segments.first(), Some(&layer), "{code}: wrong layer");
    }
    assert!(!message.is_empty(), "{code}: no message");
}
