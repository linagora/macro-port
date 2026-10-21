// SPDX-License-Identifier: Apache-2.0
//! `parse_project` on arbitrary bytes, under a code page that the input chooses
//! (or none, as when the `dir` stream gives none): the text of the `PROJECT`
//! stream, its `GC` property and the data encryption of MS-OVBA. The input is
//! the choice of the code page (1 byte, then 2 if there is one) and the stream.
//! Reading never fails, so the check is that it returns.

#![no_main]

use std::hint::black_box;

use libfuzzer_sys::fuzz_target;
use macroport_core::ovba::parse_project;

fuzz_target!(|input: (Option<u16>, &[u8])| {
    let (code_page, stream) = input;
    black_box(parse_project(stream, code_page));
});
