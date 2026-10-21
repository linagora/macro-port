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

/// What the relationships of a workbook whose VBA part has moved say about it.
#[derive(Clone, Copy)]
enum Link {
    /// They still name `vbaProject.bin`, which is no longer there.
    Broken,
    /// They name the new place, `custom.dat`.
    Followed,
}

/// `workbook` with its VBA part under another name, `xl/custom.dat`.
fn with_vba_moved(workbook: &[u8], link: Link) -> Vec<u8> {
    let mut source = ZipArchive::new(Cursor::new(workbook)).unwrap();
    let mut parts = Vec::new();
    for index in 0..source.len() {
        let mut part = source.by_index(index).unwrap();
        let mut content = Vec::new();
        part.read_to_end(&mut content).unwrap();
        let mut name = part.name().unwrap().into_owned();
        if name == VBA {
            name = "xl/custom.dat".to_owned();
        } else if matches!(link, Link::Followed) && name.ends_with(".rels") {
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
        let lost = with_vba_moved(&workbook, Link::Broken);
        assert!(read_ooxml(&lost).unwrap().vba_project.is_none(), "{path}");
        let related = with_vba_moved(&workbook, Link::Followed);
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
