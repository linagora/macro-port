// SPDX-License-Identifier: Apache-2.0
//! `CfbFile::open` on arbitrary bytes, then `read_stream` on the streams the
//! analysis reads: the VBA project of a workbook (`.xls`) and that of an OOXML
//! package (`vbaProject.bin`), the BIFF workbook, which is large enough to
//! leave the mini stream, and the streams of an encrypted OOXML package.

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::container::CfbFile;
use macroport_fuzz::assert_typed_error;

const STREAMS: [&str; 8] = [
    "_VBA_PROJECT_CUR/VBA/dir",
    "_VBA_PROJECT_CUR/PROJECT",
    "VBA/dir",
    "PROJECT",
    "VBA/_VBA_PROJECT",
    "Workbook",
    "EncryptionInfo",
    "EncryptedPackage",
];

fuzz_target!(|data: &[u8]| {
    let mut file = match CfbFile::open(data) {
        Ok(file) => file,
        Err(error) => {
            return assert_typed_error(Some("cfb"), error.status, error.code, &error.message);
        }
    };
    for path in STREAMS {
        match file.read_stream(path) {
            Ok(Some(bytes)) => {
                assert!(file.has_stream(path), "{path}");
                // The container does not change under a read.
                assert_eq!(file.read_stream(path).ok().flatten(), Some(bytes), "{path}");
            }
            Ok(None) => assert!(!file.has_stream(path), "{path}"),
            Err(error) => assert_typed_error(Some("cfb"), error.status, error.code, &error.message),
        }
    }
});
