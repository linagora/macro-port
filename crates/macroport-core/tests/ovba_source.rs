// SPDX-License-Identifier: Apache-2.0
//! The source of a module: text decoded from the code page of the project, and
//! the source that a module stream holds past its compiled code.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

mod ovba_common;

use macroport_core::ovba::{decode_text, module_source};
use macroport_core::report::Status;
use ovba_common::Lcg;

/// The code pages that the tests decode with, and some that name no encoding.
const CODE_PAGES: [Option<u16>; 8] = [
    None,
    Some(0),
    Some(437),
    Some(932),
    Some(1200),
    Some(1251),
    Some(1252),
    Some(65001),
];

/// The normal compression example of MS-OVBA section 3.2.
const NORMAL: &str = "01 2F B0 00 23 61 61 61 62 63 64 65 82 66 00 70 61 67 68 69 6A 01 38 \
                      08 61 6B 6C 00 20 6D 6E 6F 70 06 71 02 70 04 00 72 73 74 75 76 10 77 \
                      78 79 7A 00 2C";

/// A module in the way that the VBA editor writes it, and the text it makes.
const SOURCE: &str =
    "Attribute VB_Name = \"Module1\"\r\nSub Main()\r\n    MsgBox \"Hi\"\r\nEnd Sub\r\n";
const SOURCE_TEXT: &str =
    "Attribute VB_Name = \"Module1\"\nSub Main()\n    MsgBox \"Hi\"\nEnd Sub\n";

// Builders.

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect()
}

/// `text` as UTF-16 little-endian.
fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A compressed container that holds `data` as literal tokens. A chunk holds
/// 3,640 bytes at most: with the flag byte of every eight, that fills the
/// 4096 bytes that a chunk may take.
fn literals(data: &[u8]) -> Vec<u8> {
    let mut container = vec![0x01];
    for chunk in data.chunks(3_640) {
        let mut tokens = Vec::new();
        for group in chunk.chunks(8) {
            tokens.push(0x00);
            tokens.extend_from_slice(group);
        }
        // The size field is the size of the chunk, header included, less 3.
        let size = u16::try_from(tokens.len().saturating_sub(1)).unwrap();
        container.extend((0xB000 | size).to_le_bytes());
        container.extend(tokens);
    }
    container
}

/// A module stream: `cache` stands for the compiled code, and the source
/// follows it.
fn module_stream(cache: &[u8], source: &[u8]) -> Vec<u8> {
    [cache, &literals(source)].concat()
}

// Code pages.

#[test]
fn decodes_windows_1252() {
    assert_eq!(
        decode_text(&[0x80, 0x99, 0xE9, 0xFF], Some(1252)),
        "\u{20ac}\u{2122}\u{e9}\u{ff}"
    );
}

#[test]
fn gives_every_byte_a_character_in_windows_1252() {
    let bytes: Vec<u8> = (0..=u8::MAX).collect();
    let text = decode_text(&bytes, Some(1252));
    assert_eq!(text.chars().count(), 256);
    assert!(!text.contains('\u{fffd}'));
}

#[test]
fn decodes_windows_1251() {
    assert_eq!(
        decode_text(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2], Some(1251)),
        "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}"
    );
}

#[test]
fn decodes_windows_932() {
    // Three double-byte characters, a half-width katakana and ASCII.
    assert_eq!(
        decode_text(&[0x93, 0xFA, 0x96, 0x7B, 0x81, 0x41, 0xB1, 0x41], Some(932)),
        "\u{65e5}\u{672c}\u{3001}\u{ff71}A"
    );
    assert_eq!(
        decode_text(&[0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA], Some(932)),
        "\u{65e5}\u{672c}\u{8a9e}"
    );
}

#[test]
fn replaces_a_double_byte_sequence_that_is_cut_short() {
    // The lead byte at the end of the text.
    assert_eq!(decode_text(&[0x41, 0x93], Some(932)), "A\u{fffd}");
    // A lead byte that a line ending follows leaves the line ending alone.
    assert_eq!(
        decode_text(&[0x93, 0x0D, 0x0A, 0x41], Some(932)),
        "\u{fffd}\nA"
    );
}

#[test]
fn decodes_utf_16_for_code_page_1200() {
    // A supplementary character takes a pair of surrogates.
    let text = "Sub \u{c4}\u{20ac}\u{1f600}";
    assert_eq!(decode_text(&utf16(text), Some(1200)), text);
}

#[test]
fn replaces_what_utf_16_cannot_decode() {
    // A lone high surrogate, then a byte left over.
    assert_eq!(
        decode_text(&[0x41, 0x00, 0x00, 0xD8, 0x42, 0x00, 0x43], Some(1200)),
        "A\u{fffd}B\u{fffd}"
    );
}

#[test]
fn falls_back_to_windows_1252_for_a_code_page_it_does_not_know() {
    let bytes = [0x80, 0xE9, 0x41];
    for code_page in [None, Some(0), Some(437), Some(65_535)] {
        assert_eq!(
            decode_text(&bytes, code_page),
            "\u{20ac}\u{e9}A",
            "{code_page:?}"
        );
    }
}

#[test]
fn keeps_a_byte_order_mark() {
    // The bytes are in the code page of the project, whatever they start with.
    assert_eq!(
        decode_text(&[0xFF, 0xFE, 0x41, 0x00], Some(1200)),
        "\u{feff}A"
    );
    assert_eq!(
        decode_text(&[0xEF, 0xBB, 0xBF, 0x41], Some(1252)),
        "\u{ef}\u{bb}\u{bf}A"
    );
}

#[test]
fn decodes_nothing_to_nothing() {
    for code_page in CODE_PAGES {
        assert_eq!(decode_text(&[], code_page), "", "{code_page:?}");
    }
}

// Line endings.

#[test]
fn ends_every_line_with_a_line_feed() {
    // A carriage return and a line feed, a carriage return alone, a line feed
    // alone, a line feed and a carriage return, and the pair after a lone one.
    let cases = [
        ("a\r\nb", "a\nb"),
        ("a\rb", "a\nb"),
        ("a\nb", "a\nb"),
        ("a\n\rb", "a\n\nb"),
        ("a\r\r\nb", "a\n\nb"),
        ("a\r\n\r\nb", "a\n\nb"),
        ("a\r", "a\n"),
        ("\r\n", "\n"),
        ("\r", "\n"),
        ("\r\n\r\n\r\n", "\n\n\n"),
    ];
    for (bytes, text) in cases {
        assert_eq!(decode_text(bytes.as_bytes(), Some(1252)), text, "{bytes:?}");
    }
}

#[test]
fn ends_the_lines_of_a_utf_16_text() {
    assert_eq!(
        decode_text(&utf16("a\r\nb\rc\nd"), Some(1200)),
        "a\nb\nc\nd"
    );
}

#[test]
fn ends_a_line_at_a_line_separator_and_a_paragraph_separator() {
    // MS-VBAL section 3.2.1 lists U+2028 and U+2029 as line terminators beside
    // the carriage return and the line feed. Only the encodings of Unicode can
    // hold them, and one after a carriage return is a line of its own.
    let text = "a\u{2028}b\u{2029}c\r\u{2028}d";
    assert_eq!(decode_text(&utf16(text), Some(1200)), "a\nb\nc\n\nd");
    assert_eq!(decode_text(text.as_bytes(), Some(65001)), "a\nb\nc\n\nd");
    // The next line character of Unicode is not one of them.
    assert_eq!(decode_text(&utf16("a\u{85}b"), Some(1200)), "a\u{85}b");
}

#[test]
fn ends_the_lines_in_every_code_page() {
    for code_page in CODE_PAGES {
        let bytes = if code_page == Some(1200) {
            utf16("a\r\nb")
        } else {
            b"a\r\nb".to_vec()
        };
        assert_eq!(decode_text(&bytes, code_page), "a\nb", "{code_page:?}");
    }
}

// The source of a module stream.

#[test]
fn reads_the_source_that_follows_the_compiled_code() {
    let stream = module_stream(&[0xAA; 600], SOURCE.as_bytes());
    assert_eq!(
        module_source(&stream, 600, Some(1252), 1 << 20).unwrap(),
        SOURCE_TEXT
    );
}

#[test]
fn reads_from_the_offset_and_not_before() {
    // The compiled code happens to be a container itself.
    let cache = literals(b"compiled code");
    let stream = module_stream(&cache, b"Sub Main()\r\nEnd Sub\r\n");
    let offset = u32::try_from(cache.len()).unwrap();
    assert_eq!(
        module_source(&stream, offset, Some(1252), 1024).unwrap(),
        "Sub Main()\nEnd Sub\n"
    );
}

#[test]
fn reads_a_stream_that_has_no_compiled_code() {
    let stream = module_stream(&[], SOURCE.as_bytes());
    assert_eq!(
        module_source(&stream, 0, Some(1252), 1 << 20).unwrap(),
        SOURCE_TEXT
    );
}

#[test]
fn decodes_the_source_in_the_code_page_of_the_project() {
    let stream = module_stream(&[], &[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2]);
    assert_eq!(module_source(&stream, 0, Some(1251), 64).unwrap(), "Привет");
    assert_eq!(module_source(&stream, 0, None, 64).unwrap(), "Ïðèâåò");
    let stream = module_stream(&[], &utf16("Sub Ä\r\n"));
    assert_eq!(
        module_source(&stream, 0, Some(1200), 64).unwrap(),
        "Sub Ä\n"
    );
}

#[test]
fn reads_a_source_of_several_chunks() {
    let source = "Debug.Print \"0123456789\"\r\n".repeat(1_000);
    let stream = module_stream(&[0xAA; 10], source.as_bytes());
    assert_eq!(
        module_source(&stream, 10, Some(1252), 1 << 20).unwrap(),
        source.replace("\r\n", "\n")
    );
}

#[test]
fn reads_the_copy_tokens_of_a_container() {
    let stream = [vec![0xAA; 5], hex(NORMAL)].concat();
    assert_eq!(
        module_source(&stream, 5, None, 1024).unwrap(),
        "#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa"
    );
}

#[test]
fn a_stream_that_ends_at_the_offset_has_no_source() {
    assert_eq!(module_source(&[1, 2, 3], 3, Some(1252), 64).unwrap(), "");
    assert_eq!(module_source(&[], 0, Some(1252), 64).unwrap(), "");
}

#[test]
fn a_container_without_chunks_has_no_source() {
    assert_eq!(module_source(&[0xAA, 0x01], 1, None, 64).unwrap(), "");
}

#[test]
fn refuses_an_offset_past_the_end_of_the_stream() {
    for offset in [4, 5, 1 << 20, u32::MAX] {
        let error = module_source(&[1, 2, 3], offset, Some(1252), 64).unwrap_err();
        assert_eq!(error.code, "ovba.module.bad_offset", "{offset}");
        assert_eq!(error.status, Status::Corrupted, "{offset}");
    }
}

#[test]
fn passes_on_the_errors_of_decompress() {
    let code = |tail: &[u8]| {
        let stream = [&[0xAA; 4], tail].concat();
        module_source(&stream, 4, None, 64).unwrap_err().code
    };
    // The signature byte is not 0x01.
    assert_eq!(code(&[0x02, 0x00]), "ovba.decompress.bad_signature");
    // A chunk header is cut short.
    assert_eq!(code(&[0x01, 0x19]), "ovba.decompress.truncated");
    // A chunk header has no signature.
    assert_eq!(code(&[0x01, 0x00, 0x00]), "ovba.decompress.bad_chunk");
}

#[test]
fn bounds_the_size_of_the_decompressed_source() {
    let stream = module_stream(&[], &[b'a'; 5_000]);
    assert_eq!(module_source(&stream, 0, None, 5_000).unwrap().len(), 5_000);
    let error = module_source(&stream, 0, None, 4_999).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.too_large");
    assert_eq!(error.status, Status::SuspiciousArchive);
    // The caller is told what the read decompressed before it failed.
    assert_eq!(error.produced, 5_000);
}

#[test]
fn counts_the_bytes_of_the_source_and_not_those_of_the_text() {
    // Each of these bytes makes a character of 3 bytes in UTF-8.
    let stream = module_stream(&[], &[0x80; 1_000]);
    assert_eq!(
        module_source(&stream, 0, Some(1252), 1_000).unwrap().len(),
        3_000
    );
}

// Hostile input.

#[test]
fn never_panics_on_random_text_and_leaves_only_line_feeds() {
    let mut lcg = Lcg::new(0x0BAD_CAFE);
    for _ in 0..1_000 {
        for code_page in CODE_PAGES {
            let text = decode_text(&lcg.random_bytes(), code_page);
            assert!(
                !text.contains(['\r', '\u{2028}', '\u{2029}']),
                "{code_page:?}"
            );
        }
    }
}

#[test]
fn never_panics_on_damaged_or_random_streams() {
    let valid = module_stream(&[0xAA; 40], SOURCE.as_bytes());
    let mut lcg = Lcg::new(0x1F2E_3D4C);
    for round in 0..3_000_u32 {
        let damaged = lcg.damage(&valid, 4);
        for offset in [0, 40, round % 90, u32::MAX] {
            let _ = module_source(&damaged, offset, Some(1252), 1 << 16);
        }
        let _ = module_source(&lcg.random_bytes(), round % 8, Some(932), 1 << 16);
    }
}
