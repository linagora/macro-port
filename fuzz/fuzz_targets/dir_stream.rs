// SPDX-License-Identifier: Apache-2.0
//! The `dir` stream parser on arbitrary bytes, read as a decompressed stream
//! (`parse_dir`) and as a compressed one (`parse_compressed_dir`).

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::ovba::{
    DirInfo, MAX_MODULES, MAX_REFERENCES, OvbaError, parse_compressed_dir, parse_dir,
};
use macroport_fuzz::assert_typed_error;

fn check(result: Result<DirInfo, OvbaError>) {
    match result {
        Ok(info) => {
            assert!(info.modules.len() <= MAX_MODULES);
            assert!(info.references.len() <= MAX_REFERENCES);
            for module in &info.modules {
                assert!(!module.name.is_empty() && !module.stream_name.is_empty());
            }
            // SPEC M1, item 7: a reference keeps its display name, never a path.
            for reference in &info.references {
                assert!(!reference.is_empty() && !reference.contains(['/', '\\']));
            }
        }
        Err(error) => assert_typed_error(Some("ovba"), error.status, error.code, &error.message),
    }
}

fuzz_target!(|data: &[u8]| {
    check(parse_dir(data));
    check(parse_compressed_dir(data));
});
