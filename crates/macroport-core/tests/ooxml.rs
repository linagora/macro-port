// SPDX-License-Identifier: Apache-2.0
//! The OOXML reader: the VBA part of a package, found through its
//! relationships, its content types or its name, and its Excel 4.0 macro
//! sheets, within the limits of SPEC M1.

// `clippy.toml` lets `#[test]` functions panic, not the helpers they call.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used
)]

use std::io::{Cursor, Read, Write};

use macroport_core::container::{ContainerError, read_ooxml};
use macroport_core::report::Container;
use macroport_core::report::Status::{self, Corrupted, Encrypted, SuspiciousArchive, Unsupported};
use macroport_core::rules::{RuleSet, RulesSources};
use macroport_testkit::corpus;
use serde::Deserialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MIB: usize = 1024 * 1024;
const VBA: &str = "xl/vbaProject.bin";
const RELS_NAMESPACE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const TYPES_NAMESPACE: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const VBA_RELATIONSHIP: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
const MACRO_SHEET_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/xlMacrosheet";
const VBA_TYPE: &str = "application/vnd.ms-office.vbaProject";
const MACRO_SHEET_TYPE: &str = "application/vnd.ms-excel.macrosheet+xml";
const BINARY_MACRO_SHEET_TYPE: &str = "application/vnd.ms-excel.macrosheet";
const WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const IN_XLSX: &str = "sec.vbaproject-in-xlsx";
const MACRO_SHEET_INDICATOR: &str = "sec.xlm-macrosheet";

/// A zip whose entries are `(name, content, method)`. A name that ends with a
/// slash is a directory, and its content is ignored.
fn zip_of(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content, method) in entries {
        let mut options = SimpleFileOptions::default().compression_method(*method);
        if *method == CompressionMethod::Deflated {
            options = options.compression_level(Some(9));
        }
        if name.ends_with('/') {
            writer.add_directory(*name, options).unwrap();
            continue;
        }
        writer.start_file(*name, options).unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A `.rels` part with this XML between the tags of its root.
fn rels_with(body: &str) -> String {
    format!(r#"<Relationships xmlns="{RELS_NAMESPACE}">{body}</Relationships>"#)
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
    rels_with(&relationships)
}

/// A package from the bytes of the relationships of the package and of those
/// of its workbook, `xl/workbook.xml`, with these other parts.
fn package_of(root: &[u8], workbook: &[u8], parts: &[(&str, &[u8])]) -> Vec<u8> {
    let deflated = CompressionMethod::Deflated;
    let mut entries = vec![
        ("_rels/.rels", root, deflated),
        ("xl/workbook.xml", b"<workbook/>".as_slice(), deflated),
        ("xl/_rels/workbook.xml.rels", workbook, deflated),
    ];
    entries.extend(
        parts
            .iter()
            .map(|(name, content)| (*name, *content, deflated)),
    );
    zip_of(&entries)
}

/// A package whose workbook is `xl/workbook.xml`, with these relationships of
/// the workbook as `(type, target)` and these other parts.
fn workbook_package(relationships: &[(&str, &str)], parts: &[(&str, &[u8])]) -> Vec<u8> {
    let workbook = rels_of(relationships);
    package_of(root_rels().as_bytes(), workbook.as_bytes(), parts)
}

/// A package whose only relationships part, at the root, is `root`.
fn package_with_root(root: &[u8]) -> Vec<u8> {
    zip_of(&[("_rels/.rels", root, CompressionMethod::Deflated)])
}

/// A package whose relationships at the root are `root`, and whose workbook
/// relates to `xl/custom.dat`. The VBA part is also there under its usual
/// name, so that the part the reader returns tells whether it read `root`.
fn related_package(root: &[u8]) -> Vec<u8> {
    let workbook = rels_of(&[(VBA_RELATIONSHIP, "custom.dat")]);
    let parts: [(&str, &[u8]); 2] = [("xl/custom.dat", b"by relationship"), (VBA, b"by name")];
    package_of(root, workbook.as_bytes(), &parts)
}

/// The relationships of a package whose workbook is `xl/workbook.xml`.
fn root_rels() -> String {
    rels_of(&[(OFFICE_DOCUMENT, "xl/workbook.xml")])
}

/// A `[Content_Types].xml` part with this XML between the tags of its root.
fn types_with(body: &str) -> String {
    format!(r#"<Types xmlns="{TYPES_NAMESPACE}">{body}</Types>"#)
}

/// A `[Content_Types].xml` part with these `(extension, type)` defaults and
/// these `(part name, type)` overrides.
fn types_of(defaults: &[(&str, &str)], overrides: &[(&str, &str)]) -> String {
    let default = |(extension, kind): &(&str, &str)| {
        format!(r#"<Default Extension="{extension}" ContentType="{kind}"/>"#)
    };
    let over = |(part, kind): &(&str, &str)| {
        format!(r#"<Override PartName="{part}" ContentType="{kind}"/>"#)
    };
    let defaults = defaults.iter().map(default);
    types_with(
        &defaults
            .chain(overrides.iter().map(over))
            .collect::<String>(),
    )
}

/// `workbook_package` with this `[Content_Types].xml`.
fn typed_package(types: &str, relationships: &[(&str, &str)], parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut parts = parts.to_vec();
    parts.push(("[Content_Types].xml", types.as_bytes()));
    workbook_package(relationships, &parts)
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

/// What leads to the VBA part of a workbook once it has moved.
#[derive(Clone, Copy)]
enum Link {
    /// Nothing: the relationships still name `vbaProject.bin`, and neither the
    /// content types nor the new name, `xl/custom.dat`, lead to the part.
    Broken,
    /// The relationships name the new place, `xl/custom.dat`.
    Followed,
    /// The content types type the new place, `xl/custom.bin`, as a VBA project
    /// (the type of `.bin` in an `.xlsm`, an override in an `.xlsb`), and the
    /// relationships still name `vbaProject.bin`.
    Typed,
}

/// `workbook` with its VBA part under another name.
fn with_vba_moved(workbook: &[u8], link: Link) -> Vec<u8> {
    let mut source = ZipArchive::new(Cursor::new(workbook)).unwrap();
    let mut parts = Vec::new();
    for index in 0..source.len() {
        let mut part = source.by_index(index).unwrap();
        let mut content = Vec::new();
        part.read_to_end(&mut content).unwrap();
        let mut name = part.name().unwrap().into_owned();
        if name == VBA {
            let new = if matches!(link, Link::Typed) {
                "xl/custom.bin"
            } else {
                "xl/custom.dat"
            };
            name = new.to_owned();
        } else if matches!(link, Link::Followed) && name.ends_with(".rels") {
            let rels = String::from_utf8(content).unwrap();
            let moved = rels.replace(r#"Target="vbaProject.bin""#, r#"Target="custom.dat""#);
            content = moved.into_bytes();
        } else if matches!(link, Link::Typed) && name == "[Content_Types].xml" {
            let types = String::from_utf8(content).unwrap();
            let moved = types.replace(
                r#"PartName="/xl/vbaProject.bin""#,
                r#"PartName="/xl/custom.bin""#,
            );
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
/// workbook (`workbook.xml.rels` in `.xlsm`, `workbook.bin.rels` in `.xlsb`),
/// or only its content types, lead to it.
#[test]
fn every_generated_workbook_leads_to_its_vba_project_by_relationship_and_by_type() {
    for path in workbooks_with_vba() {
        let workbook = corpus::read(&path);
        // Moved and not led to any more, the part is lost: otherwise its name
        // would find it and the test would prove nothing.
        let lost = with_vba_moved(&workbook, Link::Broken);
        assert!(read_ooxml(&lost).unwrap().vba_project.is_none(), "{path}");
        let expected = vba_of(&workbook);
        for link in [Link::Followed, Link::Typed] {
            assert_eq!(vba_of(&with_vba_moved(&workbook, link)), expected, "{path}");
        }
    }
}

#[test]
fn a_part_typed_as_a_vba_project_is_found_without_a_relationship() {
    // However the type is given: for the part or for its extension, with the
    // case and the parameters it may have.
    let cases = [
        types_of(&[], &[("/xl/custom.dat", VBA_TYPE)]),
        types_of(&[], &[("xl/custom.dat", VBA_TYPE)]),
        types_of(&[], &[("/XL/Custom.DAT", VBA_TYPE)]),
        types_of(&[("dat", VBA_TYPE)], &[]),
        types_of(&[("DAT", VBA_TYPE)], &[]),
        types_of(&[("dat", "APPLICATION/VND.MS-OFFICE.VBAPROJECT")], &[]),
        types_of(
            &[("dat", " application/vnd.ms-office.vbaProject ; x=1")],
            &[],
        ),
        types_of(&[("dat", "text/plain")], &[("/xl/custom.dat", VBA_TYPE)]),
    ];
    let parts: [(&str, &[u8]); 1] = [("xl/custom.dat", b"by type")];
    for types in &cases {
        let package = typed_package(types, &[], &parts);
        assert_eq!(vba_of(&package), b"by type", "{types}");
    }
}

#[test]
fn a_part_not_typed_as_a_vba_project_is_not_taken_for_one() {
    let signature = "application/vnd.ms-office.vbaProjectSignature";
    let cases = [
        types_of(&[("dat", "text/plain")], &[]),
        types_of(&[("bin", VBA_TYPE)], &[]),
        types_of(&[("dat", signature)], &[]),
        types_of(&[("dat", "application/vnd.ms-office.vbaProject+xml")], &[]),
        // An override of another type replaces the type of the extension.
        types_of(&[("dat", VBA_TYPE)], &[("/xl/custom.dat", "text/plain")]),
        // A type given to a part that is not there.
        types_of(&[], &[("/xl/missing.dat", VBA_TYPE)]),
    ];
    let parts: [(&str, &[u8]); 1] = [("xl/custom.dat", b"by type")];
    for types in &cases {
        let package = typed_package(types, &[], &parts);
        assert!(
            read_ooxml(&package).unwrap().vba_project.is_none(),
            "{types}"
        );
    }
}

#[test]
fn the_vba_part_is_found_by_relationship_then_by_type_then_by_name() {
    let types = types_of(&[], &[("/xl/typed.dat", VBA_TYPE)]);
    let parts: [(&str, &[u8]); 3] = [
        ("xl/custom.dat", b"by relationship"),
        ("xl/typed.dat", b"by type"),
        (VBA, b"by name"),
    ];
    let related = [(VBA_RELATIONSHIP, "custom.dat")];
    let by_relationship = typed_package(&types, &related, &parts);
    assert_eq!(vba_of(&by_relationship), b"by relationship");
    assert_eq!(vba_of(&typed_package(&types, &[], &parts)), b"by type");
    let untyped = types_of(&[], &[]);
    assert_eq!(vba_of(&typed_package(&untyped, &[], &parts)), b"by name");
}

#[test]
fn of_several_parts_typed_as_a_vba_project_only_the_first_is_read() {
    // The first by name is the project. The others are not even inflated, as
    // the one of them that is a bomb shows.
    let types = types_of(&[("dat", VBA_TYPE)], &[]);
    let bomb = vec![0; 20 * MIB];
    let parts: [(&str, &[u8]); 3] = [
        ("xl/b.dat", &bomb),
        ("xl/a.dat", b"first"),
        ("xl/c.dat", &bomb),
    ];
    assert_eq!(vba_of(&typed_package(&types, &[], &parts)), b"first");
}

#[test]
fn content_types_that_are_not_what_the_format_says_are_corrupted() {
    let ns = TYPES_NAMESPACE;
    let cases = [
        format!(r#"<Types xmlns="{ns}">"#),
        "<Types/>".to_owned(),
        format!(r#"<Relationships xmlns="{ns}"/>"#),
        types_with(r#"<Default Extension="a" ContentType="x" ContentType="y"/>"#),
        types_with(r#"<Override xmlns="urn:evil" PartName="/a" ContentType="x"/>"#),
        types_with(r#"<Override PartName="/a" ContentType="x""#),
    ];
    for types in &cases {
        let package = typed_package(types, &[], &[]);
        assert_fails(&package, Corrupted, "ooxml.malformed_xml");
    }
}

#[test]
fn content_types_over_their_caps_are_a_suspicious_archive() {
    let (stored, deflated) = (CompressionMethod::Stored, CompressionMethod::Deflated);
    let root = root_rels();
    let package = |types: &[u8], method| {
        zip_of(&[
            ("_rels/.rels", root.as_bytes(), deflated),
            ("[Content_Types].xml", types, method),
        ])
    };
    let over = package(&vec![b' '; 16 * MIB + 1], stored);
    assert_fails(&over, SuspiciousArchive, "zip.entry_too_large");
    let mut types = types_of(&[], &[]).into_bytes();
    types.resize(16 * MIB, b' ');
    assert!(read_ooxml(&package(&types, stored)).is_ok());
    let bomb = package(&vec![b' '; 20 * MIB], deflated);
    assert_fails(&bomb, SuspiciousArchive, "zip.ratio");
}

fn macro_sheets_of(bytes: &[u8]) -> u32 {
    read_ooxml(bytes).unwrap().macro_sheets
}

/// The expectations of `fixtures.yaml` that the macro sheet tests use.
#[derive(Deserialize)]
struct Fixtures {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    file: String,
    expected_xlm: Option<ExpectedXlm>,
}

#[derive(Deserialize)]
struct ExpectedXlm {
    macro_sheets: u32,
}

#[test]
fn the_macro_sheets_of_the_fixtures_are_counted() {
    let text = String::from_utf8(corpus::read("fixtures/fixtures.yaml")).unwrap();
    // The file starts with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let manifest: Fixtures = serde_saphyr::from_str(text).unwrap();
    let (mut packages, mut with_sheets) = (0, 0);
    for fixture in manifest.fixtures {
        let bytes = corpus::read(&format!("fixtures/{}", fixture.file));
        // The `.xls` workbooks and the encrypted package are compound files.
        if !bytes.starts_with(b"PK") {
            continue;
        }
        // A fixture that says nothing about Excel 4.0 macros has none.
        let expected = fixture.expected_xlm.map_or(0, |xlm| xlm.macro_sheets);
        assert_eq!(macro_sheets_of(&bytes), expected, "{}", fixture.file);
        packages += 1;
        with_sheets += u32::from(expected > 0);
    }
    assert!(packages >= 150, "only {packages} packages");
    assert!(with_sheets >= 1, "no package with macro sheets");
}

#[test]
fn macro_sheets_are_counted_through_content_types_and_relationships() {
    let parts: [(&str, &[u8]); 4] = [
        ("xl/macrosheets/sheet1.xml", b"<macro/>"),
        ("xl/macrosheets/sheet2.xml", b"<macro/>"),
        ("xl/macrosheets/sheet3.bin", b"binary"),
        ("xl/worksheets/sheet1.xml", b"<sheet/>"),
    ];
    let sheet1 = ("/xl/macrosheets/sheet1.xml", MACRO_SHEET_TYPE);
    let by_type = types_of(&[], &[sheet1]);
    let none = types_of(&[], &[]);
    let related = |target: &'static str| (MACRO_SHEET_RELATIONSHIP, target);
    let other = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let (worksheet, international) = (
        format!("{other}/worksheet"),
        format!("{other}/xlIntlMacrosheet"),
    );
    let cases = [
        ("a type", by_type.clone(), vec![], 1),
        (
            "the binary type",
            types_of(
                &[],
                &[("/xl/macrosheets/sheet3.bin", BINARY_MACRO_SHEET_TYPE)],
            ),
            vec![],
            1,
        ),
        (
            "a type, in any case, with parameters",
            types_of(
                &[],
                &[(
                    "/xl/macrosheets/sheet1.xml",
                    "Application/VND.MS-Excel.MacroSheet+XML ; x=1",
                )],
            ),
            vec![],
            1,
        ),
        (
            "an extension, but not for the parts that have another type",
            types_of(
                &[("xml", MACRO_SHEET_TYPE)],
                &[
                    ("/xl/worksheets/sheet1.xml", WORKSHEET_TYPE),
                    ("/xl/workbook.xml", WORKSHEET_TYPE),
                ],
            ),
            vec![],
            2,
        ),
        (
            "a relationship",
            none.clone(),
            vec![related("macrosheets/sheet1.xml")],
            1,
        ),
        (
            "a relationship type in any case",
            none.clone(),
            vec![("http://example.com/XLMACROSHEET", "macrosheets/sheet1.xml")],
            1,
        ),
        (
            "a type and a relationship, for one part",
            by_type.clone(),
            vec![related("macrosheets/sheet1.xml")],
            1,
        ),
        (
            "a type and a relationship, for two parts",
            by_type.clone(),
            vec![related("macrosheets/sheet2.xml")],
            2,
        ),
        (
            "a relationship to a part that is not there",
            none.clone(),
            vec![related("macrosheets/missing.xml")],
            0,
        ),
        (
            "a type for a part that is not there",
            types_of(&[], &[("/xl/macrosheets/missing.xml", MACRO_SHEET_TYPE)]),
            vec![],
            0,
        ),
        (
            "other types of sheets",
            types_of(&[], &[("/xl/worksheets/sheet1.xml", WORKSHEET_TYPE)]),
            vec![
                (worksheet.as_str(), "worksheets/sheet1.xml"),
                (international.as_str(), "macrosheets/sheet1.xml"),
            ],
            0,
        ),
    ];
    for (label, types, relationships, expected) in cases {
        let package = typed_package(&types, &relationships, &parts);
        assert_eq!(macro_sheets_of(&package), expected, "{label}");
    }
}

#[test]
fn a_macro_sheet_related_to_the_package_or_not_internal_is_counted_or_not() {
    let parts: [(&str, &[u8]); 1] = [("xl/macrosheets/sheet1.xml", b"<macro/>")];
    // The workbook is not the only part whose relationships count.
    let root = rels_of(&[
        (OFFICE_DOCUMENT, "xl/workbook.xml"),
        (MACRO_SHEET_RELATIONSHIP, "xl/macrosheets/sheet1.xml"),
    ]);
    let workbook = rels_of(&[]);
    let package = package_of(root.as_bytes(), workbook.as_bytes(), &parts);
    assert_eq!(macro_sheets_of(&package), 1);
    // An address is not a part, even when a part has that name.
    let external = format!(
        r#"<Relationship Id="x" Type="{MACRO_SHEET_RELATIONSHIP}" Target="macrosheets/sheet1.xml" TargetMode="External"/>"#
    );
    let workbook = rels_with(&external);
    let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &parts);
    assert_eq!(macro_sheets_of(&package), 0);
}

#[test]
fn a_macro_sheet_is_counted_without_being_read() {
    // Inflated, this part would be a suspicious archive.
    let bomb = vec![0; 20 * MIB];
    let types = types_of(&[], &[("/xl/macrosheets/sheet1.xml", MACRO_SHEET_TYPE)]);
    let parts: [(&str, &[u8]); 1] = [("xl/macrosheets/sheet1.xml", &bomb)];
    assert_eq!(macro_sheets_of(&typed_package(&types, &[], &parts)), 1);
}

#[test]
fn a_directory_of_the_archive_is_not_a_part() {
    // A folder entry has no content, so no type makes it a sheet or a project.
    let types = types_of(
        &[],
        &[
            ("/xl/macrosheets/", MACRO_SHEET_TYPE),
            ("/xl/project/", VBA_TYPE),
        ],
    );
    let folders: [(&str, &[u8]); 2] = [("xl/macrosheets/", b""), ("xl/project/", b"")];
    let package = read_ooxml(&typed_package(&types, &[], &folders)).unwrap();
    assert_eq!(package.macro_sheets, 0);
    assert!(package.vba_project.is_none());
}

#[test]
fn percent_escapes_in_a_target_name_the_part_that_they_decode_to() {
    // The entry as the archive names it and the target as the relationship
    // writes it: one part, whichever way each of them is spelled.
    let cases = [
        ("xl/my vba.bin", "my%20vba.bin"),
        ("xl/my%20vba.bin", "my%20vba.bin"),
        ("xl/my%20vba.bin", "my vba.bin"),
        ("xl/my vba.bin", "%6dy%20vba.bin"),
        ("xl/my vba.bin", "%6Dy%20vba.bin"),
        ("xl/my vba.bin", "/xl/my%20vba.bin"),
        ("xl/my vba.bin", "../xl/my%20vba.bin"),
        ("xl/some dir/vba.bin", "some%20dir/vba.bin"),
        ("xl/é.bin", "%C3%A9.bin"),
        ("xl/%c3%a9.bin", "é.bin"),
    ];
    for (entry, target) in cases {
        let parts: [(&str, &[u8]); 2] = [(entry, b"by relationship"), (VBA, b"by name")];
        let package = workbook_package(&[(VBA_RELATIONSHIP, target)], &parts);
        assert_eq!(vba_of(&package), b"by relationship", "{entry} and {target}");
    }
}

#[test]
fn percent_escapes_in_the_content_types_name_the_part_that_they_decode_to() {
    let projects = [
        ("xl/my vba.bin", "/xl/my%20vba.bin"),
        ("xl/my%20vba.bin", "/xl/my vba.bin"),
        ("xl/my%20vba.bin", "/XL/MY%20VBA.BIN"),
    ];
    for (entry, part_name) in projects {
        let types = types_of(&[], &[(part_name, VBA_TYPE)]);
        let parts: [(&str, &[u8]); 1] = [(entry, b"by type")];
        let package = typed_package(&types, &[], &parts);
        assert_eq!(vba_of(&package), b"by type", "{entry} and {part_name}");
    }
    let sheets = [
        (
            "xl/macrosheets/my sheet.xml",
            "/xl/macrosheets/my%20sheet.xml",
        ),
        (
            "xl/macrosheets/my%20sheet.xml",
            "/xl/macrosheets/my sheet.xml",
        ),
    ];
    for (entry, part_name) in sheets {
        let types = types_of(&[], &[(part_name, MACRO_SHEET_TYPE)]);
        let parts: [(&str, &[u8]); 1] = [(entry, b"<macro/>")];
        let package = typed_package(&types, &[], &parts);
        assert_eq!(macro_sheets_of(&package), 1, "{entry} and {part_name}");
    }
    // A relationship names a macro sheet the same way.
    let related = [(MACRO_SHEET_RELATIONSHIP, "macrosheets/my%20sheet.xml")];
    let parts: [(&str, &[u8]); 1] = [("xl/macrosheets/my sheet.xml", b"<macro/>")];
    assert_eq!(macro_sheets_of(&workbook_package(&related, &parts)), 1);
}

#[test]
fn escapes_that_decode_to_no_name_stay_as_written() {
    // The entry, the target, and whether the target leads to the entry.
    let cases = [
        // Bytes that are not UTF-8 are no character, and U+00FF is not 0xFF.
        ("xl/%ff.bin", "%ff.bin", true),
        ("xl/%FF.bin", "%ff.bin", true),
        ("xl/ÿ.bin", "%ff.bin", false),
        ("xl/%c3.bin", "%c3.bin", true),
        // Not a pair of hexadecimal digits.
        ("xl/100%.bin", "100%.bin", true),
        ("xl/%zz.bin", "%zz.bin", true),
        ("xl/%2.bin", "%2.bin", true),
        // A slash or a backslash is never made by an escape.
        ("xl/a%2fb.bin", "a%2Fb.bin", true),
        ("xl/a/b.bin", "a%2fb.bin", false),
        ("xl/a%5cb.bin", "a%5Cb.bin", true),
        ("xl/a/b.bin", "a%5cb.bin", false),
        // An escape is decoded once, not again in what it decodes to.
        ("xl/a%2520b.bin", "a%2520b.bin", true),
        ("xl/a%20b.bin", "a%2520b.bin", false),
        ("xl/a b.bin", "a%2520b.bin", false),
    ];
    for (entry, target, found) in cases {
        let parts: [(&str, &[u8]); 2] = [(entry, b"by relationship"), (VBA, b"by name")];
        let package = workbook_package(&[(VBA_RELATIONSHIP, target)], &parts);
        let expected: &[u8] = if found {
            b"by relationship"
        } else {
            b"by name"
        };
        assert_eq!(vba_of(&package), expected, "{entry} and {target}");
    }
}

#[test]
fn a_part_found_by_its_type_is_not_decoded_a_second_time() {
    // `a%2541.bin` is the part `a%41.bin`, not `aA.bin`: the decoy.
    let types = types_of(&[], &[("/xl/a%2541.bin", VBA_TYPE)]);
    let parts: [(&str, &[u8]); 2] = [("xl/a%2541.bin", b"by type"), ("xl/aa.bin", b"decoy")];
    assert_eq!(vba_of(&typed_package(&types, &[], &parts)), b"by type");
}

#[test]
fn two_entries_that_name_one_part_through_an_escape_are_ambiguous() {
    let pairs = [
        ("xl/a b.bin", "xl/a%20b.bin"),
        ("xl/a.bin", "xl/%61.bin"),
        ("xl/A.bin", "xl/%61.BIN"),
    ];
    for (first, second) in pairs {
        let parts: [(&str, &[u8]); 2] = [(first, b"1"), (second, b"2")];
        let package = workbook_package(&[], &parts);
        assert_fails(&package, Corrupted, "zip.duplicate_entry");
    }
    // `%25` is a percent sign, so these two are not one part.
    let parts: [(&str, &[u8]); 2] = [("xl/a%2520b.bin", b"1"), ("xl/a%20b.bin", b"2")];
    assert!(read_ooxml(&workbook_package(&[], &parts)).is_ok());
}

#[test]
fn the_content_types_part_is_not_a_part() {
    // A type given to every `.xml` part does not reach `[Content_Types].xml`.
    let types = types_of(&[("xml", MACRO_SHEET_TYPE)], &[]);
    let package = typed_package(&types, &[], &[]);
    // Only `xl/workbook.xml` is left.
    assert_eq!(macro_sheets_of(&package), 1);
}

#[test]
fn a_package_without_content_types_has_no_typed_parts() {
    let parts: [(&str, &[u8]); 1] = [("xl/macrosheets/sheet1.xml", b"<macro/>")];
    assert_eq!(macro_sheets_of(&workbook_package(&[], &parts)), 0);
}

#[test]
fn a_vba_part_in_a_package_declared_as_xlsx_raises_an_indicator() {
    let package = read_ooxml(&fixture("malformed/macros-in-xlsx.xlsx")).unwrap();
    assert!(package.vba_project.is_some());
    for declared in [Container::Xlsx, Container::Xltx] {
        assert_eq!(package.indicators(declared), [IN_XLSX], "{declared:?}");
    }
    for declared in [
        Container::Xlsm,
        Container::Xltm,
        Container::Xlam,
        Container::Xlsb,
    ] {
        assert!(package.indicators(declared).is_empty(), "{declared:?}");
    }
}

#[test]
fn macro_sheets_raise_an_indicator_and_a_plain_workbook_none() {
    let xlm = read_ooxml(&fixture("generated/xlm-01-macrosheet.xlsm")).unwrap();
    assert_eq!(xlm.indicators(Container::Xlsm), [MACRO_SHEET_INDICATOR]);
    let plain = read_ooxml(&fixture("generated/none-01-no-macros.xlsm")).unwrap();
    assert!(plain.indicators(Container::Xlsx).is_empty());
    // Both, in the order of `rules/security.yaml`.
    let types = types_of(&[], &[("/xl/macrosheets/sheet1.xml", MACRO_SHEET_TYPE)]);
    let parts: [(&str, &[u8]); 2] = [("xl/macrosheets/sheet1.xml", b"<macro/>"), (VBA, b"vba")];
    let both = read_ooxml(&typed_package(&types, &[], &parts)).unwrap();
    assert_eq!(
        both.indicators(Container::Xlsx),
        [MACRO_SHEET_INDICATOR, IN_XLSX]
    );
    assert_eq!(both.indicators(Container::Xlsm), [MACRO_SHEET_INDICATOR]);
}

#[test]
fn the_indicators_are_those_of_the_shipped_rules_in_their_order() {
    let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
    let known: Vec<&str> = (rules.security.container_indicators.iter())
        .map(|indicator| indicator.id.as_str())
        .collect();
    let position = |id: &str| known.iter().position(|&known| known == id);
    assert!(position(MACRO_SHEET_INDICATOR).is_some(), "{known:?}");
    assert!(position(IN_XLSX).is_some(), "{known:?}");
    assert!(
        position(MACRO_SHEET_INDICATOR) < position(IN_XLSX),
        "{known:?}"
    );
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
    // A `..` above the root is dropped, as RFC 3986 section 5.2.4 has it.
    let found: &[&str] = &[
        "custom/p.bin",
        "./custom/p.bin",
        "../xl/custom/p.bin",
        "../../xl/custom/p.bin",
        "/xl/custom/p.bin",
        "..\\xl\\custom\\p.bin",
        "Custom/P.BIN",
        "custom/p.bin#fragment",
    ];
    // A target that names no part, or the root of the package, leaves it to
    // the name.
    let not_found: &[&str] = &["custom/missing.bin", "", ".", "..", "/", "#custom/p.bin"];
    for (targets, expected) in [(found, "by relationship"), (not_found, "by name")] {
        for target in targets {
            let package = workbook_package(&[(VBA_RELATIONSHIP, target)], &parts);
            assert_eq!(vba_of(&package), expected.as_bytes(), "{target:?}");
        }
    }
}

/// The parts of the decoy tests: the one that a decoy relationship leads to, the
/// one that the real relationship leads to, and the one the name gives.
const DECOY_PARTS: [(&str, &[u8]); 3] = [
    ("xl/decoy.bin", b"decoy"),
    ("xl/custom.dat", b"by relationship"),
    (VBA, b"by name"),
];

#[test]
fn a_vba_relationship_counts_by_its_exact_type() {
    // Each decoy comes before the real relationship and leads to a part that
    // exists. The type is compared as written, with its case and as a whole,
    // not by what it ends with.
    let decoys = [
        "http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature",
        "http://schemas.microsoft.com/office/2006/relationships/VBAPROJECT",
        "HTTP://schemas.microsoft.com/office/2006/relationships/vbaProject",
        "http://example.com/relationships/vbaProject",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
    ];
    for decoy in decoys {
        let relationships = [(decoy, "decoy.bin"), (VBA_RELATIONSHIP, "custom.dat")];
        let package = workbook_package(&relationships, &DECOY_PARTS);
        assert_eq!(vba_of(&package), b"by relationship", "{decoy}");
    }
    let untyped = r#"<Relationship Id="r0" Target="decoy.bin"/>"#;
    let real = format!(r#"<Relationship Id="r1" Type="{VBA_RELATIONSHIP}" Target="custom.dat"/>"#);
    let workbook = rels_with(&format!("{untyped}{real}"));
    let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
    assert_eq!(vba_of(&package), b"by relationship");
}

#[test]
fn a_workbook_relationship_counts_by_its_exact_type() {
    // The decoy workbook relates to a decoy VBA part and comes first. The
    // Strict variant of the format has another type, which the reader does not
    // know: its workbook is not followed.
    let decoys = [
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/OFFICEDOCUMENT",
        "http://example.com/relationships/officeDocument",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
        "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument",
    ];
    for decoy in decoys {
        let root = rels_of(&[
            (decoy, "xl/decoy.xml"),
            (OFFICE_DOCUMENT, "xl/workbook.xml"),
        ]);
        let decoy_rels = rels_of(&[(VBA_RELATIONSHIP, "decoy.bin")]);
        let mut parts = DECOY_PARTS.to_vec();
        parts.push(("xl/decoy.xml", b"<workbook/>"));
        parts.push(("xl/_rels/decoy.xml.rels", decoy_rels.as_bytes()));
        let workbook = rels_of(&[(VBA_RELATIONSHIP, "custom.dat")]);
        let package = package_of(root.as_bytes(), workbook.as_bytes(), &parts);
        assert_eq!(vba_of(&package), b"by relationship", "{decoy}");
    }
}

#[test]
fn an_external_relationship_is_not_followed() {
    // The target of an external relationship is an address, not a part, even
    // when a part of that name exists.
    let relationship = |id: &str, target: &str, mode: &str| {
        let mode = if mode.is_empty() {
            String::new()
        } else {
            format!(r#" TargetMode="{mode}""#)
        };
        format!(r#"<Relationship Id="{id}" Type="{VBA_RELATIONSHIP}" Target="{target}"{mode}/>"#)
    };
    let external = relationship("r0", "decoy.bin", "External");
    for mode in ["Internal", ""] {
        let internal = relationship("r1", "custom.dat", mode);
        let workbook = rels_with(&format!("{external}{internal}"));
        let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
        assert_eq!(vba_of(&package), b"by relationship", "{mode:?}");
    }
}

#[test]
fn an_unknown_target_mode_is_corrupted_where_the_reader_looks() {
    // Only the two modes of the format are known, and compared as written: the
    // reader cannot tell what Excel makes of another one. It matters only on a
    // relationship of a type the reader needs.
    for mode in ["external", "EXTERNAL", "other", ""] {
        let odd = format!(
            r#"<Relationship Id="r0" Type="{VBA_RELATIONSHIP}" Target="decoy.bin" TargetMode="{mode}"/>"#
        );
        let workbook = rels_with(&odd);
        let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
        assert_fails(&package, Corrupted, "ooxml.malformed_xml");
    }
    let hyperlink = r#"<Relationship Id="r0" Type="http://example.com/hyperlink" Target="x" TargetMode="odd"/>"#;
    let workbook = rels_with(hyperlink);
    let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
    assert_eq!(vba_of(&package), b"by name");
}

#[test]
fn the_workbook_is_the_first_one_that_exists() {
    let missing = (OFFICE_DOCUMENT, "xl/missing.xml");
    let root = rels_of(&[missing, (OFFICE_DOCUMENT, "xl/workbook.xml")]);
    assert_eq!(
        vba_of(&related_package(root.as_bytes())),
        b"by relationship"
    );
}

#[test]
fn the_package_itself_is_no_workbook() {
    // `.` is the root of the package, and the relationships of the package are
    // not those of a workbook: the VBA relationship there leads nowhere.
    let stored = CompressionMethod::Stored;
    for target in [".", "", "/", "..", "#xl/workbook.xml"] {
        let root = rels_of(&[
            (OFFICE_DOCUMENT, target),
            (VBA_RELATIONSHIP, "xl/decoy.bin"),
        ]);
        let package = zip_of(&[
            ("_rels/.rels", root.as_bytes(), stored),
            // An entry that stands for the root, to look up by the empty name.
            ("/", b"", stored),
            ("xl/decoy.bin", b"decoy", stored),
            (VBA, b"by name", stored),
        ]);
        assert_eq!(vba_of(&package), b"by name", "{target:?}");
    }
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
    let package = package_of(root.as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
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
    let (broken, deflated) = ("<Relationships><", CompressionMethod::Deflated);
    let in_package = zip_of(&[("_rels/.rels", broken.as_bytes(), deflated)]);
    let in_workbook = package_of(root_rels().as_bytes(), broken.as_bytes(), &[]);
    for package in [in_package, in_workbook] {
        assert_fails(&package, Corrupted, "ooxml.malformed_xml");
    }
}

#[test]
fn a_relationship_part_that_is_not_what_the_format_says_is_corrupted() {
    let vba = VBA_RELATIONSHIP;
    let ns = RELS_NAMESPACE;
    let evil = r#"xmlns:x="urn:evil""#;
    let spelled = ns.replacen(':', "&#58;", 1);
    let cases = [
        // Not well formed.
        String::new(),
        format!(r#"<Relationships xmlns="{ns}">"#),
        format!(r#"<Relationships xmlns="{ns}"></Relationships><Relationships xmlns="{ns}"/>"#),
        "</Relationships>".to_owned(),
        format!(r#"<Relationships xmlns="{ns}"/></Relationships>"#),
        format!(r#"<Relationships xmlns="{ns}"><Relationship></Relationships>"#),
        format!(r#"<Relationships xmlns="{ns}"><Relationship Target=a/></Relationships>"#),
        format!("<Relationships xmlns=\"{ns}\">\u{0}</Relationships"),
        // Not what the format has: a DTD, a repeated attribute.
        format!(r#"<!DOCTYPE Relationships []><Relationships xmlns="{ns}"/>"#),
        rels_with(&format!(
            r#"<Relationship Type="{vba}" Target="a" Target="b"/>"#
        )),
        rels_with(r#"<Relationship Type="x" Type="y"/>"#),
        // Not in the namespace of the format, at the root or among its children,
        // or written so that only a parser that decodes it would see it.
        "<Relationships/>".to_owned(),
        format!(r#"<Other xmlns="{ns}"/>"#),
        r#"<Relationships xmlns="urn:evil"/>"#.to_owned(),
        format!(r#"<Relationships xmlns="{spelled}"/>"#),
        rels_with(&format!(
            r#"<x:Relationship {evil} Type="{vba}" Target="a"/>"#
        )),
        rels_with(&format!(
            r#"<Relationship xmlns="urn:evil" Type="{vba}" Target="a"/>"#
        )),
        // Text where only the root may be.
        format!(r#"junk<Relationships xmlns="{ns}"/>"#),
        format!(r#"<Relationships xmlns="{ns}"/>junk"#),
        format!(r#"<![CDATA[x]]><Relationships xmlns="{ns}"/>"#),
        format!(r#"&amp;<Relationships xmlns="{ns}"/>"#),
    ];
    for xml in &cases {
        let error = error_of(&package_with_root(xml.as_bytes()));
        let found = (error.status, error.code);
        assert_eq!(found, (Corrupted, "ooxml.malformed_xml"), "{xml}");
    }
    let error = error_of(&package_with_root(&[0xc3, 0x28]));
    assert_eq!(error.code, "ooxml.malformed_xml");
}

#[test]
fn a_relationship_part_in_the_namespace_of_the_format_is_read() {
    let real = format!(r#"<Relationship Id="1" Type="{VBA_RELATIONSHIP}" Target="custom.dat"/>"#);
    let prefixed = format!(
        r#"<r:Relationships xmlns:r="{RELS_NAMESPACE}"><r:Relationship Id="1" Type="{VBA_RELATIONSHIP}" Target="custom.dat"/></r:Relationships>"#
    );
    let declared = format!(
        "\n<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<!-- c -->\n{}\n",
        rels_with(&real)
    );
    // The names of attributes count with their prefix: this decoy has no `Type`.
    let decoy = format!(
        r#"<Relationship xmlns:x="urn:x" Id="0" x:Type="{VBA_RELATIONSHIP}" Target="decoy.bin"/>"#
    );
    let with_decoy = rels_with(&format!("{decoy}{real}"));
    for workbook in [prefixed, declared, with_decoy] {
        let package = package_of(root_rels().as_bytes(), workbook.as_bytes(), &DECOY_PARTS);
        assert_eq!(vba_of(&package), b"by relationship", "{workbook}");
    }
}

#[test]
fn a_relationship_part_is_read_as_utf8_with_or_without_a_byte_order_mark() {
    let root = root_rels();
    assert_eq!(
        vba_of(&related_package(root.as_bytes())),
        b"by relationship"
    );
    let marked = [&[0xef, 0xbb, 0xbf], root.as_bytes()].concat();
    assert_eq!(vba_of(&related_package(&marked)), b"by relationship");
    // Another encoding is not read: UTF-16 is not UTF-8.
    let units = root.encode_utf16().flat_map(u16::to_le_bytes);
    let utf16: Vec<u8> = [0xff, 0xfe].into_iter().chain(units).collect();
    assert_fails(&related_package(&utf16), Corrupted, "ooxml.malformed_xml");
}

#[test]
fn a_relationship_part_nested_too_deeply_is_corrupted() {
    // Two levels are the format's; the reader stops at 32.
    let nested = |depth: usize| {
        let (open, close) = ("<a>".repeat(depth), "</a>".repeat(depth));
        rels_with(&format!("{open}{close}"))
    };
    assert!(read_ooxml(&package_with_root(nested(31).as_bytes())).is_ok());
    let deep = package_with_root(nested(32).as_bytes());
    assert_fails(&deep, Corrupted, "ooxml.malformed_xml");
}

#[test]
fn a_flood_of_namespace_declarations_is_corrupted() {
    // The parser bounds the declarations it keeps, so a part cannot spend
    // memory on them beyond its own size.
    let declarations = |count: usize| {
        let declared: String = (0..count)
            .map(|n| format!(r#" xmlns:p{n}="urn:{n}""#))
            .collect();
        format!(r#"<Relationships xmlns="{RELS_NAMESPACE}"{declared}/>"#)
    };
    assert!(read_ooxml(&package_with_root(declarations(20).as_bytes())).is_ok());
    let flood = package_with_root(declarations(1000).as_bytes());
    assert_fails(&flood, Corrupted, "ooxml.malformed_xml");
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
        rels_of(&[
            (VBA_RELATIONSHIP, "vbaProject.bin"),
            (MACRO_SHEET_RELATIONSHIP, "macrosheets/sheet1.xml"),
        ]),
        types_of(
            &[("bin", VBA_TYPE)],
            &[("/xl/macrosheets/sheet1.xml", MACRO_SHEET_TYPE)],
        ),
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
        let others: [(&str, &[u8]); 3] = [
            (VBA, b"vba"),
            ("xl/macrosheets/sheet1.xml", b"<macro/>"),
            ("[Content_Types].xml", &parts[2]),
        ];
        let package = package_of(&parts[0], &parts[1], &others);
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
