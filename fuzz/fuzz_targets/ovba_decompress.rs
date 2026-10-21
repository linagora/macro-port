// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA decompression of arbitrary bytes, under a limit on the output that
//! the input chooses, so that short inputs reach both sides of the limit. The
//! input is the limit (2 bytes) followed by the compressed container.

#![no_main]

use libfuzzer_sys::fuzz_target;
use macroport_core::ovba::decompress;
use macroport_fuzz::assert_typed_error;

/// More than any limit the input can choose (`u16::MAX`).
const LARGE_LIMIT: usize = 1 << 20;

fuzz_target!(|input: (u16, &[u8])| {
    let (limit, container) = input;
    let limit = usize::from(limit);
    match decompress(container, limit) {
        Ok(output) => {
            assert!(output.len() <= limit);
            // The limit only says when to give up: a container that fits under
            // it decompresses to the same bytes under a larger one.
            assert!(decompress(container, LARGE_LIMIT) == Ok(output));
        }
        Err(error) => assert_typed_error(Some("ovba"), error.status, error.code, &error.message),
    }
});
