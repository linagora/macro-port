// SPDX-License-Identifier: Apache-2.0
//! `module_source` on arbitrary bytes: the source of a module, which follows
//! the compiled code in its stream at an offset, is decompressed and decoded in
//! the code page of the project. The input is the offset (2 bytes), the choice
//! of the code page (1 byte, then 2 if there is one) and the stream.

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::ovba::module_source;
use macroport_fuzz::assert_typed_error;

/// What a source may decompress to: the largest in the fixtures has 152 KB.
const MAX_SOURCE: usize = 1 << 18;

fuzz_target!(|input: (u16, Option<u16>, &[u8])| {
    let (offset, code_page, stream) = input;
    match module_source(stream, offset.into(), code_page, MAX_SOURCE) {
        Ok(source) => {
            // Every line ends with a line feed, and a byte makes three at most.
            assert!(!source.contains(['\r', '\u{2028}', '\u{2029}']));
            assert!(source.len() <= 3 * MAX_SOURCE);
        }
        Err(error) => assert_typed_error(Some("ovba"), error.status, error.code, &error.message),
    }
});
