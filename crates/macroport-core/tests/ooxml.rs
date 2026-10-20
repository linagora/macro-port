// SPDX-License-Identifier: Apache-2.0
//! The OOXML reader: the VBA part of a package, within the limits of SPEC M1.

// `clippy.toml` lets `#[test]` functions panic, not the helpers they call.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use std::io::{Cursor, Write};

use macroport_core::container::{ContainerError, read_ooxml};
use macroport_core::report::Status::{self, Corrupted, Encrypted, SuspiciousArchive, Unsupported};
use macroport_testkit::corpus;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MIB: usize = 1024 * 1024;
const VBA: &str = "xl/vbaProject.bin";

/// A zip whose entries are `(name, content, method)`.
fn zip_of(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content, method) in entries {
        let mut options = SimpleFileOptions::default().compression_method(*method);
        if *method == CompressionMethod::Deflated {
            options = options.compression_level(Some(9));
        }
        writer.start_file(*name, options).unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A zip with one deflated entry, the VBA part.
fn deflated(content: &[u8]) -> Vec<u8> {
    zip_of(&[(VBA, content, CompressionMethod::Deflated)])
}

fn fixture(relative: &str) -> Vec<u8> {
    corpus::read(&format!("fixtures/{relative}"))
}

fn error_of(bytes: &[u8]) -> ContainerError {
    read_ooxml(bytes).unwrap_err()
}

fn vba_of(bytes: &[u8]) -> Vec<u8> {
    read_ooxml(bytes).unwrap().vba_project.expect("a VBA part")
}

/// Asserts that reading `bytes` fails with this status and code.
fn assert_fails(bytes: &[u8], status: Status, code: &str) {
    let error = error_of(bytes);
    assert_eq!((error.status, error.code), (status, code), "{error}");
}

/// Overwrites a field of the only entry of a zip, in its local header at
/// offset `local` and in its central directory entry at offset `central`. The
/// offsets are those of APPNOTE 4.3.7 and 4.3.12: in the local header the
/// flags are at 6, the method at 8, the CRC at 14 and the sizes at 18 and 22;
/// in the central one they are at 8, 10, 16, 20 and 24.
fn patch(bytes: &mut [u8], local: usize, central: usize, value: &[u8]) {
    let local_header = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
    let central_header = bytes.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    for at in [local_header + local, central_header + central] {
        bytes[at..at + value.len()].copy_from_slice(value);
    }
}

/// A deflate stream that holds `content` in stored blocks: it inflates at a
/// ratio of 1, however long it is.
fn stored_blocks(content: &[u8]) -> Vec<u8> {
    let mut stream = Vec::new();
    let mut blocks = content.chunks(0xffff).peekable();
    while let Some(block) = blocks.next() {
        let length = u16::try_from(block.len()).unwrap();
        stream.push(u8::from(blocks.peek().is_none()));
        stream.extend_from_slice(&length.to_le_bytes());
        stream.extend_from_slice(&(!length).to_le_bytes());
        stream.extend_from_slice(block);
    }
    stream
}

#[test]
fn every_generated_macro_workbook_gives_its_vba_project() {
    let mut read = 0;
    for path in corpus::files("fixtures/generated", &["xlsm", "xlsb"]) {
        let name = path.rsplit('/').next().unwrap();
        if matches!(name.split('-').next(), Some("enc" | "none" | "xlm")) {
            continue;
        }
        let vba = vba_of(&corpus::read(&path));
        assert!(vba.starts_with(&CFB_MAGIC), "{path}: not a compound file");
        read += 1;
    }
    assert!(read >= 100, "only {read} workbooks read");
}

#[test]
fn a_package_without_a_vba_part_has_none() {
    for name in ["none-01-no-macros.xlsm", "xlm-01-macrosheet.xlsm"] {
        let workbook = fixture(&format!("generated/{name}"));
        assert!(
            read_ooxml(&workbook).unwrap().vba_project.is_none(),
            "{name}"
        );
    }
    assert!(read_ooxml(&zip_of(&[])).unwrap().vba_project.is_none());
}

#[test]
fn an_encrypted_workbook_is_not_a_zip() {
    let workbook = fixture("generated/enc-01-open-password.xlsm");
    assert_fails(&workbook, Unsupported, "zip.not_a_zip");
}

#[test]
fn malformed_fixtures_give_typed_errors() {
    let cases = [
        ("empty.xlsm", Unsupported, "zip.not_a_zip"),
        ("random-bytes.xlsm", Unsupported, "zip.not_a_zip"),
        ("text-renamed.xlsm", Unsupported, "zip.not_a_zip"),
        ("truncated-half.xlsm", Corrupted, "zip.invalid_archive"),
        ("truncated-tail.xlsm", Corrupted, "zip.invalid_archive"),
        ("zip-bomb.xlsm", SuspiciousArchive, "zip.ratio"),
    ];
    for (name, status, code) in cases {
        let error = error_of(&fixture(&format!("malformed/{name}")));
        assert_eq!((error.status, error.code), (status, code), "{name}");
    }
}

#[test]
fn damage_inside_the_vba_part_is_left_to_the_compound_file_reader() {
    assert!(!vba_of(&fixture("malformed/vbaproject-not-cfb.xlsm")).starts_with(&CFB_MAGIC));
    assert!(vba_of(&fixture("malformed/vbaproject-bitflips.xlsm")).starts_with(&CFB_MAGIC));
    assert!(vba_of(&fixture("malformed/macros-in-xlsx.xlsx")).starts_with(&CFB_MAGIC));
}

#[test]
fn a_prefix_of_a_workbook_is_never_readable_and_never_panics() {
    let workbook = fixture("generated/01-01-hide-unhide-columns-find.xlsm");
    for length in 0..workbook.len() {
        let error = error_of(&workbook[..length]);
        assert!(
            matches!(error.status, Corrupted | Unsupported),
            "{length}: {error}"
        );
    }
}

#[test]
fn the_vba_part_is_found_by_name_whatever_the_case() {
    let package = zip_of(&[("XL/VbaProject.BIN", b"vba", CompressionMethod::Deflated)]);
    assert_eq!(vba_of(&package), b"vba");
}

#[test]
fn two_names_that_differ_by_case_are_ambiguous() {
    let deflated = CompressionMethod::Deflated;
    let package = zip_of(&[
        (VBA, b"one", deflated),
        ("XL/VBAPROJECT.BIN", b"two", deflated),
    ]);
    assert_fails(&package, Corrupted, "zip.duplicate_entry");
}

#[test]
fn a_multi_disk_archive_is_unsupported() {
    let mut package = deflated(b"vba");
    let end_record = package.len() - 22;
    package[end_record + 4] = 1;
    assert_fails(&package, Unsupported, "zip.unsupported");
}

#[test]
fn a_stored_vba_part_over_the_cap_is_a_suspicious_archive() {
    let package = zip_of(&[(VBA, &vec![7; 65 * MIB], CompressionMethod::Stored)]);
    assert_fails(&package, SuspiciousArchive, "zip.entry_too_large");
}

#[test]
fn a_deflated_vba_part_over_the_cap_is_a_suspicious_archive() {
    let stream = stored_blocks(&vec![7; 65 * MIB]);
    let mut package = zip_of(&[(VBA, &stream, CompressionMethod::Stored)]);
    patch(&mut package, 8, 10, &8u16.to_le_bytes());
    assert_fails(&package, SuspiciousArchive, "zip.entry_too_large");
}

#[test]
fn a_vba_part_of_exactly_the_cap_is_read() {
    let package = zip_of(&[(VBA, &vec![7; 64 * MIB], CompressionMethod::Stored)]);
    assert_eq!(vba_of(&package).len(), 64 * MIB);
}

#[test]
fn a_part_that_inflates_beyond_its_ratio_is_a_suspicious_archive() {
    let mut package = deflated(&vec![0; 20 * MIB]);
    assert_fails(&package, SuspiciousArchive, "zip.ratio");
    // The sizes in the headers are not trusted: they now say 10 bytes.
    patch(&mut package, 22, 24, &10u32.to_le_bytes());
    assert_fails(&package, SuspiciousArchive, "zip.ratio");
}

#[test]
fn a_damaged_header_gives_a_typed_error() {
    // Where to patch (in the local header, in the central one), what to write,
    // and the error that follows.
    let cases: [(usize, usize, &[u8], Status, &str); 4] = [
        (6, 8, &[1, 0], Encrypted, "zip.encrypted_entry"),
        (8, 10, &[12, 0], Unsupported, "zip.unsupported_method"),
        (14, 16, &[7, 0, 0, 0], Corrupted, "zip.crc_mismatch"),
        (18, 20, &[0, 0, 0, 9], Corrupted, "zip.entry_out_of_bounds"),
    ];
    for (local, central, value, status, code) in cases {
        let mut package = deflated(b"vba");
        patch(&mut package, local, central, value);
        assert_fails(&package, status, code);
    }
}

#[test]
fn a_damaged_deflate_stream_is_corrupted() {
    let mut package = deflated(&vec![1; 4096]);
    let data = 30 + VBA.len();
    package[data..data + 2].copy_from_slice(&[0xff, 0xff]);
    assert_fails(&package, Corrupted, "zip.inflate");
}

#[test]
fn two_entries_of_exactly_one_name_are_ambiguous() {
    // `ZipWriter` refuses the same name twice: the second entry is renamed
    // once written.
    let deflated = CompressionMethod::Deflated;
    let mut package = zip_of(&[
        (VBA, b"one", deflated),
        ("xl/vbaProject.bio", b"two", deflated),
    ]);
    for at in 0..package.len() {
        if package[at..].starts_with(b"xl/vbaProject.bio") {
            package[at + 16] = b'n';
        }
    }
    assert_fails(&package, Corrupted, "zip.duplicate_entry");
}

#[test]
fn a_huge_central_directory_is_a_suspicious_archive() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for index in 0..65_000 {
        writer
            .start_file(format!("{index:0>200}"), options)
            .unwrap();
    }
    let package = writer.finish().unwrap().into_inner();
    assert_fails(&package, SuspiciousArchive, "zip.directory_too_large");
}

#[test]
fn changed_bytes_never_panic() {
    let workbook = fixture("generated/01-01-hide-unhide-columns-find.xlsm");
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state >> 8).unwrap()
    };
    for _ in 0..3000 {
        let mut bytes = workbook.clone();
        for _ in 0..=next() % 4 {
            // Half of the changes are among the last bytes, where the central
            // directory is.
            let reach = if next() % 2 == 0 { bytes.len() } else { 2048 };
            let at = bytes.len() - 1 - next() % reach;
            bytes[at] = next().to_le_bytes()[0];
        }
        let _ = read_ooxml(&bytes);
    }
}
