// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA decompression: the worked examples of the specification, the
//! rules it sets on a container, and hostile input.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

mod ovba_common;

use macroport_core::ovba::decompress;
use macroport_core::report::Status;
use ovba_common::Lcg;

/// The worked examples of MS-OVBA section 3.2, compressed.
const NO_COMPRESSION: &str = "01 19 B0 00 61 62 63 64 65 66 67 68 00 69 6A 6B 6C 6D 6E 6F 70 \
                              00 71 72 73 74 75 76 2E";
const NORMAL: &str = "01 2F B0 00 23 61 61 61 62 63 64 65 82 66 00 70 61 67 68 69 6A 01 38 \
                      08 61 6B 6C 00 20 6D 6E 6F 70 06 71 02 70 04 00 72 73 74 75 76 10 77 \
                      78 79 7A 00 2C";
/// One literal and a copy of 72 bytes.
const MAXIMUM: &str = "01 03 B0 02 61 45 00";

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

fn error_code(input: &str, max_output: usize) -> &'static str {
    decompress(&hex(input), max_output).unwrap_err().code
}

// The worked examples of MS-OVBA section 3.2.

#[test]
fn decompresses_the_example_without_compression() {
    assert_eq!(
        decompress(&hex(NO_COMPRESSION), 1024).unwrap(),
        b"abcdefghijklmnopqrstuv."
    );
}

#[test]
fn decompresses_the_normal_example() {
    assert_eq!(
        decompress(&hex(NORMAL), 1024).unwrap(),
        b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa"
    );
}

#[test]
fn decompresses_the_maximum_compression_example() {
    assert_eq!(decompress(&hex(MAXIMUM), 1024).unwrap(), vec![b'a'; 73]);
}

// Containers.

#[test]
fn a_container_without_chunks_is_empty() {
    assert_eq!(decompress(&[0x01], 0).unwrap(), b"");
}

#[test]
fn copies_a_raw_chunk_as_is() {
    let payload: Vec<u8> = (0..4096_u32).map(|index| (index % 251) as u8).collect();
    let mut container = vec![0x01, 0xFF, 0x3F];
    container.extend_from_slice(&payload);
    assert_eq!(decompress(&container, 4096).unwrap(), payload);
}

#[test]
fn accepts_a_chunk_of_4096_bytes() {
    // One literal and a copy of 4095 bytes.
    let output = decompress(&hex("01 03 B0 02 61 FC 0F"), 4096).unwrap();
    assert_eq!(output, vec![b'a'; 4096]);
}

#[test]
fn joins_the_chunks_and_never_copies_across_them() {
    // The second chunk starts again with a literal.
    let mut container = hex(MAXIMUM);
    container.extend(hex("03 B0 02 62 45 00"));
    let mut expected = vec![b'a'; 73];
    expected.extend(vec![b'b'; 73]);
    assert_eq!(decompress(&container, 146).unwrap(), expected);
}

#[test]
fn reads_a_short_chunk_that_is_not_the_last() {
    // Section 2.4.1.1.2 asks writers for 4096 bytes in every chunk but the
    // last. The reading algorithm of section 2.4.1.3 does not check it.
    assert_eq!(
        decompress(&hex("01 01 B0 00 61 01 B0 00 62"), 1024).unwrap(),
        b"ab"
    );
}

#[test]
fn reads_a_chunk_up_to_the_end_of_a_container_that_cuts_it_short() {
    // The header announces 6 bytes of data and 4 follow. Section 2.4.1.3.2
    // ends a chunk at the end of the container when its size runs past it, so
    // the chunk is read as far as it goes: here, the maximum compression
    // example.
    assert_eq!(
        decompress(&hex("01 05 B0 02 61 45 00"), 1024).unwrap(),
        vec![b'a'; 73]
    );
}

#[test]
fn reads_a_token_sequence_without_tokens() {
    // A flag byte alone: section 2.4.1.1.7 asks writers for one token at
    // least in the last sequence, which the reading algorithm does not check.
    assert_eq!(decompress(&hex("01 00 B0 00"), 1024).unwrap(), b"");
}

// Invalid containers.

#[test]
fn rejects_an_empty_input() {
    let error = decompress(&[], 1024).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.truncated");
    assert_eq!(error.status, Status::Corrupted);
}

#[test]
fn rejects_a_wrong_signature_byte() {
    let error = decompress(&hex("02 03 B0 02 61 45 00"), 1024).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.bad_signature");
    assert_eq!(error.status, Status::Corrupted);
}

#[test]
fn rejects_a_chunk_header_cut_short() {
    assert_eq!(error_code("01 03", 1024), "ovba.decompress.truncated");
}

#[test]
fn rejects_a_chunk_whose_signature_bits_are_wrong() {
    // 0xA003: compressed, with the signature 010 instead of 011.
    let error = decompress(&hex("01 03 A0 02 61 45 00"), 1024).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.bad_chunk");
    assert_eq!(error.status, Status::Corrupted);
}

#[test]
fn rejects_a_raw_chunk_that_is_not_4096_bytes() {
    assert_eq!(
        error_code("01 03 30 61 61 61 61", 1024),
        "ovba.decompress.bad_chunk"
    );
}

#[test]
fn rejects_a_raw_chunk_that_the_container_cuts_short() {
    // The header of a raw chunk, then 100 of its 4096 bytes.
    let mut container = vec![0x01, 0xFF, 0x3F];
    container.extend([b'a'; 100]);
    let error = decompress(&container, 4096).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.truncated");
    assert_eq!(error.status, Status::Corrupted);
}

#[test]
fn rejects_a_container_that_ends_inside_a_copy_token() {
    // The header announces 6 bytes of data and 3 follow, the last one the
    // first half of a copy token. The end of the container cuts the token,
    // not the chunk: this is not a bad token.
    assert_eq!(
        error_code("01 05 B0 02 61 45", 1024),
        "ovba.decompress.truncated"
    );
}

#[test]
fn rejects_a_copy_token_cut_short() {
    // The data ends after the first byte of the copy token.
    assert_eq!(
        error_code("01 02 B0 02 61 45", 1024),
        "ovba.decompress.bad_token"
    );
}

#[test]
fn rejects_a_copy_token_at_the_start_of_a_chunk() {
    assert_eq!(
        error_code("01 02 B0 01 45 00", 1024),
        "ovba.decompress.bad_token"
    );
}

#[test]
fn rejects_a_copy_that_reaches_before_the_chunk() {
    // After one byte the offset takes 4 bits; 0x1000 asks for offset 2.
    assert_eq!(
        error_code("01 03 B0 02 61 00 10", 1024),
        "ovba.decompress.bad_token"
    );
}

#[test]
fn rejects_a_copy_that_reaches_into_the_previous_chunk() {
    // The second chunk holds one literal, then copies from an offset of 2.
    assert_eq!(
        error_code("01 01 B0 00 61 03 B0 02 62 00 10", 1024),
        "ovba.decompress.bad_token"
    );
}

#[test]
fn rejects_a_chunk_that_decompresses_beyond_4096_bytes() {
    // One literal and a copy of 4096 bytes make 4097, the first one too
    // many; a copy of 4098 bytes makes 4099.
    for copy in ["FD 0F", "FF 0F"] {
        assert_eq!(
            error_code(&format!("01 03 B0 02 61 {copy}"), usize::MAX),
            "ovba.decompress.bad_chunk"
        );
    }
}

// Limits.

#[test]
fn accepts_exactly_the_output_limit() {
    assert_eq!(decompress(&hex(MAXIMUM), 73).unwrap().len(), 73);
}

#[test]
fn rejects_an_output_beyond_the_limit() {
    let error = decompress(&hex(MAXIMUM), 72).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.too_large");
    assert_eq!(error.status, Status::SuspiciousArchive);
}

#[test]
fn stops_at_the_chunk_that_crosses_the_limit() {
    // 100 chunks of 73 bytes.
    let mut container = vec![0x01];
    for _ in 0..100 {
        container.extend(hex("03 B0 02 61 45 00"));
    }
    let error = decompress(&container, 1000).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.too_large");
    assert_eq!(decompress(&container, 7300).unwrap().len(), 7300);
}

#[test]
fn an_error_displays_its_code_then_its_message() {
    let error = decompress(&[], 1024).unwrap_err();
    assert_eq!(
        error.to_string(),
        "ovba.decompress.truncated: the container is empty"
    );
}

// Hostile input.

#[test]
fn a_container_cut_short_gives_a_prefix_of_its_output_or_is_truncated() {
    for example in [NO_COMPRESSION, NORMAL, MAXIMUM] {
        let example = hex(example);
        let whole = decompress(&example, 1024).unwrap();
        for length in 0..example.len() {
            // A cut between two tokens gives the bytes before it. A cut
            // inside a token or a header, or an empty input, is truncated.
            match decompress(example.get(..length).unwrap(), 1024) {
                Ok(output) => assert!(whole.starts_with(&output), "length {length}"),
                Err(error) => {
                    assert_eq!(error.code, "ovba.decompress.truncated", "length {length}");
                }
            }
        }
    }
}

#[test]
fn never_panics_on_corrupted_or_random_bytes() {
    let mut lcg = Lcg::new(0x9E37_79B9);
    let valid = hex(NORMAL);
    for _ in 0..2_000 {
        let _ = decompress(&lcg.damage(&valid, 3), 1 << 16);
        // Random bytes after the signature byte.
        let random = [vec![0x01], lcg.random_bytes()].concat();
        let _ = decompress(&random, 1 << 16);
    }
}
