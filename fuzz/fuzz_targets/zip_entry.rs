// SPDX-License-Identifier: Apache-2.0
//! `read_ooxml` on arbitrary bytes: the ZIP central directory, the lookup of
//! the VBA part and its bounded inflate (SPEC M1).

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::container::read_ooxml;
use macroport_fuzz::assert_typed_error;

/// SPEC M1: the most that is inflated from the VBA part, 64 MiB.
const MAX_VBA_PART: usize = 64 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    match read_ooxml(data) {
        Ok(package) => {
            if let Some(vba_project) = package.vba_project {
                assert!(vba_project.len() <= MAX_VBA_PART);
            }
        }
        // No layer is named: the OOXML reader may add codes of its own.
        Err(error) => assert_typed_error(None, error.status, error.code, &error.message),
    }
});
