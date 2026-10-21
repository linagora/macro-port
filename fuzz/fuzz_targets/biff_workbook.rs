// SPDX-License-Identifier: Apache-2.0
//! `read_biff_workbook` on arbitrary bytes, read as the `Workbook` stream of an
//! `.xls` file: records cut short, a FILEPASS record, and BoundSheet8 records
//! too short to hold their type.

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::container::read_biff_workbook;
use macroport_fuzz::assert_typed_error;

/// The record type that opens a workbook (BOF), as the stream writes it.
const BOF: [u8; 2] = 0x0809_u16.to_le_bytes();

fuzz_target!(|data: &[u8]| {
    match read_biff_workbook(data) {
        Ok(info) => {
            assert!(data.starts_with(&BOF));
            // Only an encrypted workbook hides the types of its sheets.
            assert_eq!(info.file_pass, info.macro_sheets.is_none());
        }
        Err(error) => assert_typed_error(Some("biff"), error.status, error.code, &error.message),
    }
});
