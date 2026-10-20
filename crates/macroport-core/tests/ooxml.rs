// SPDX-License-Identifier: Apache-2.0
//! The OOXML reader: the VBA part of a package, found through its
//! relationships, within the limits of SPEC M1.

// `clippy.toml` lets `#[test]` functions panic, not the helpers they call.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use std::io::{Cursor, Read, Write};

use macroport_core::container::{ContainerError, read_ooxml};
use macroport_core::report::Status::{self, Corrupted, Encrypted, SuspiciousArchive, Unsupported};
use macroport_testkit::corpus;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MIB: usize = 1024 * 1024;
const VBA: &str = "xl/vbaProject.bin";
const RELS_NAMESPACE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const VBA_RELATIONSHIP: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

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

/// A `.rels` part with these `(type, target)` relationships.
fn rels_of(relationships: &[(&str, &str)]) -> String {
    let relationships: String = relationships
        .iter()
        .enumerate()
        .map(|(id, (kind, target))| {
            format!(r#"<Relationship Id="r{id}" Type="{kind}" Target="{target}"/>"#)
        })
        .collect();
    format!(r#"<Relationships xmlns="{RELS_NAMESPACE}">{relationships}</Relationships>"#)
}

/// A package whose workbook is `xl/workbook.xml`, with these relationships of
/// the workbook as `(type, target)` and these other parts.
fn workbook_package(relationships: &[(&str, &str)], parts: &[(&str, &[u8])]) -> Vec<u8> {
    let root = rels_of(&[(OFFICE_DOCUMENT, "xl/workbook.xml")]);
    let workbook = rels_of(relationships);
    let deflated = CompressionMethod::Deflated;
    let mut entries = vec![
        ("_rels/.rels", root.as_bytes(), deflated),
        ("xl/_rels/workbook.xml.rels", workbook.as_bytes(), deflated),
    ];
    entries.extend(
        parts
            .iter()
            .map(|(name, content)| (*name, *content, deflated)),
    );
    zip_of(&entries)
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

/// The paths of the generated workbooks that hold a VBA project and are
/// readable: not the encrypted ones, nor those without macros or with Excel 4.0
/// macros only.
fn workbooks_with_vba() -> Vec<String> {
    let workbooks = corpus::files("fixtures/generated", &["xlsm", "xlsb"]);
    let with_vba = workbooks.into_iter().filter(|path| {
        let name = path.rsplit('/').next().unwrap();
        !matches!(name.split('-').next(), Some("enc" | "none" | "xlm"))
    });
    with_vba.collect()
}

/// `workbook` with its VBA part under another name, `xl/custom.dat`. The
/// relationships keep saying `vbaProject.bin`, unless `relink` makes them say
/// `custom.dat`.
fn with_vba_moved(workbook: &[u8], relink: bool) -> Vec<u8> {
    let mut source = ZipArchive::new(Cursor::new(workbook)).unwrap();
    let mut parts = Vec::new();
    for index in 0..source.len() {
        let mut part = source.by_index(index).unwrap();
        let mut content = Vec::new();
        part.read_to_end(&mut content).unwrap();
        let mut name = part.name().unwrap().into_owned();
        if name == VBA {
            name = "xl/custom.dat".to_owned();
        } else if relink && name.ends_with(".rels") {
            let rels = String::from_utf8(content).unwrap();
            let moved = rels.replace(r#"Target="vbaProject.bin""#, r#"Target="custom.dat""#);
            content = moved.into_bytes();
        }
        parts.push((name, content));
    }
    let deflated = CompressionMethod::Deflated;
    let entries: Vec<_> = parts
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_slice(), deflated))
        .collect();
    zip_of(&entries)
}

#[test]
fn every_generated_macro_workbook_gives_its_vba_project() {
    let workbooks = workbooks_with_vba();
    assert!(workbooks.len() >= 100, "only {} workbooks", workbooks.len());
    for path in workbooks {
        let vba = vba_of(&corpus::read(&path));
        assert!(vba.starts_with(&CFB_MAGIC), "{path}: not a compound file");
    }
}

/// The fixtures all name the VBA part by its usual name, which is where the
/// reader looks anyway: this one moves it, so that only the relationship of the
/// workbook (`workbook.xml.rels` in `.xlsm`, `workbook.bin.rels` in `.xlsb`)
/// leads to it.
#[test]
fn every_generated_workbook_relates_to_its_vba_project() {
    for path in workbooks_with_vba() {
        let workbook = corpus::read(&path);
        // Moved and not related to any more, the part is lost: otherwise its
        // name would find it and the test would prove nothing.
        let lost = with_vba_moved(&workbook, false);
        assert!(read_ooxml(&lost).unwrap().vba_project.is_none(), "{path}");
        let related = with_vba_moved(&workbook, true);
        assert_eq!(vba_of(&related), vba_of(&workbook), "{path}");
    }
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
fn relationship_targets_are_resolved_as_paths() {
    let parts: [(&str, &[u8]); 2] = [("xl/custom/p.bin", b"by relationship"), (VBA, b"by name")];
    let found: &[&str] = &[
        "custom/p.bin",
        "./custom/p.bin",
        "../xl/custom/p.bin",
        "/xl/custom/p.bin",
        "..\\xl\\custom\\p.bin",
        "Custom/P.BIN",
        "custom/p.bin#fragment",
    ];
    // A target that leaves the package, or names no part, leaves it to the name.
    let not_found: &[&str] = &["../../xl/custom/p.bin", "custom/missing.bin", ""];
    for (targets, expected) in [(found, "by relationship"), (not_found, "by name")] {
        for target in targets {
            let package = workbook_package(&[(VBA_RELATIONSHIP, target)], &parts);
            assert_eq!(vba_of(&package), expected.as_bytes(), "{target:?}");
        }
    }
}

#[test]
fn a_relationship_of_another_type_does_not_name_the_vba_part() {
    // A relationship counts by its whole type, not by what the type ends with.
    let parts: [(&str, &[u8]); 2] = [("xl/decoy.xml", b"decoy"), (VBA, b"by name")];
    let other = "http://example.com/relationships/vbaProject";
    let package = workbook_package(&[(other, "decoy.xml")], &parts);
    assert_eq!(vba_of(&package), b"by name");
}

#[test]
fn a_relationship_of_the_package_does_not_name_the_vba_part() {
    // Excel relates the VBA project to its workbook, as every generated fixture
    // shows. One at the root of the package takes no precedence over it, so a
    // decoy there cannot hide the project that the workbook names.
    let root = rels_of(&[
        (OFFICE_DOCUMENT, "xl/workbook.xml"),
        (VBA_RELATIONSHIP, "xl/decoy.bin"),
    ]);
    let workbook = rels_of(&[(VBA_RELATIONSHIP, "custom.dat")]);
    let deflated = CompressionMethod::Deflated;
    let package = zip_of(&[
        ("_rels/.rels", root.as_bytes(), deflated),
        ("xl/_rels/workbook.xml.rels", workbook.as_bytes(), deflated),
        ("xl/decoy.bin", b"decoy", deflated),
        ("xl/custom.dat", b"by relationship", deflated),
    ]);
    assert_eq!(vba_of(&package), b"by relationship");
}

#[test]
fn a_relationship_to_a_very_long_name_is_not_followed() {
    for (length, expected) in [(900, "by relationship"), (2000, "by name")] {
        let long = format!("{}.bin", "d".repeat(length));
        let long_part = format!("xl/{long}");
        let parts: [(&str, &[u8]); 2] = [(&long_part, b"by relationship"), (VBA, b"by name")];
        let package = workbook_package(&[(VBA_RELATIONSHIP, &long)], &parts);
        assert_eq!(vba_of(&package), expected.as_bytes(), "{length}");
    }
}

#[test]
fn a_relationship_part_that_is_not_well_formed_is_corrupted() {
    let root = rels_of(&[(OFFICE_DOCUMENT, "xl/workbook.xml")]);
    let (broken, deflated) = ("<Relationships><", CompressionMethod::Deflated);
    let in_package = zip_of(&[("_rels/.rels", broken.as_bytes(), deflated)]);
    let in_workbook = zip_of(&[
        ("_rels/.rels", root.as_bytes(), deflated),
        ("xl/_rels/workbook.xml.rels", broken.as_bytes(), deflated),
    ]);
    for package in [in_package, in_workbook] {
        assert_fails(&package, Corrupted, "ooxml.malformed_xml");
    }
}

#[test]
fn a_malformed_value_matters_only_where_the_reader_looks() {
    // An unescaped ampersand is not valid XML. In the target of a hyperlink
    // that the reader does not need it is ignored, in that of the VBA project
    // it is not.
    let parts: [(&str, &[u8]); 1] = [("xl/custom.dat", b"by relationship")];
    let hyperlink = "http://example.com/relationships/hyperlink";
    let ignored = [
        (hyperlink, "http://example.com/?a=1&b=2"),
        (VBA_RELATIONSHIP, "custom.dat"),
    ];
    assert_eq!(
        vba_of(&workbook_package(&ignored, &parts)),
        b"by relationship"
    );
    let read = [(VBA_RELATIONSHIP, "custom&.dat")];
    assert_fails(
        &workbook_package(&read, &parts),
        Corrupted,
        "ooxml.malformed_xml",
    );
}

#[test]
fn an_xml_part_over_its_cap_is_a_suspicious_archive() {
    let stored = CompressionMethod::Stored;
    let over = zip_of(&[("_rels/.rels", &vec![b' '; 16 * MIB + 1], stored)]);
    assert_fails(&over, SuspiciousArchive, "zip.entry_too_large");
    let mut rels = rels_of(&[]).into_bytes();
    rels.resize(16 * MIB, b' ');
    let exactly = zip_of(&[("_rels/.rels", &rels, stored)]);
    assert!(read_ooxml(&exactly).is_ok());
}

#[test]
fn an_xml_part_that_inflates_beyond_its_ratio_is_a_suspicious_archive() {
    let deflated = CompressionMethod::Deflated;
    let bomb = zip_of(&[("_rels/.rels", &vec![b' '; 20 * MIB], deflated)]);
    assert_fails(&bomb, SuspiciousArchive, "zip.ratio");
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

/// A generator of numbers that is the same on every run (xorshift64).
fn numbers() -> impl FnMut() -> usize {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state >> 8).unwrap()
    }
}

#[test]
fn changed_xml_never_panics() {
    let seeds = [
        rels_of(&[(OFFICE_DOCUMENT, "xl/workbook.xml")]),
        rels_of(&[(VBA_RELATIONSHIP, "vbaProject.bin")]),
    ];
    // The characters that give XML its shape.
    let syntax = b"<>/=\"&;'! ?-[]:";
    let mut next = numbers();
    for _ in 0..3000 {
        let mut parts = seeds.clone().map(String::into_bytes);
        let part = next() % parts.len();
        for _ in 0..=next() % 4 {
            let at = next() % parts[part].len();
            parts[part][at] = match next() % 2 {
                0 => syntax[next() % syntax.len()],
                _ => next().to_le_bytes()[0],
            };
        }
        let deflated = CompressionMethod::Deflated;
        let package = zip_of(&[
            ("_rels/.rels", &parts[0], deflated),
            ("xl/_rels/workbook.xml.rels", &parts[1], deflated),
            (VBA, b"vba", deflated),
        ]);
        if let Err(error) = read_ooxml(&package) {
            assert!(
                matches!(error.status, Corrupted | SuspiciousArchive),
                "{error}"
            );
        }
    }
}

#[test]
fn changed_bytes_never_panic() {
    let workbook = fixture("generated/01-01-hide-unhide-columns-find.xlsm");
    let mut next = numbers();
    for _ in 0..3000 {
        let mut bytes = workbook.clone();
        for _ in 0..=next() % 4 {
            // Half of the changes are among the last bytes, where the central
            // directory is.
            let reach = if next().is_multiple_of(2) {
                bytes.len()
            } else {
                2048
            };
            let at = bytes.len() - 1 - next() % reach;
            bytes[at] = next().to_le_bytes()[0];
        }
        let _ = read_ooxml(&bytes);
    }
}
