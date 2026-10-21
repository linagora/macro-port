// SPDX-License-Identifier: Apache-2.0
//! The XML parts of an OOXML package, which `zip_entry` cannot reach: a part
//! that is changed no longer matches its checksum, and is refused before it is
//! read as XML. Here an archive with right checksums carries the input as one
//! of three parts (the relationships of the package, those of its workbook, or
//! the content types) and sound parts for the other two. The input is the
//! number of the part (1 byte) and its XML.

#![no_main]

use std::io::{Cursor, Write};

use libfuzzer_sys::fuzz_target;
use macroport_core::container::read_ooxml;
use macroport_fuzz::assert_typed_error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// The relationships of the package: its workbook.
const PACKAGE_RELS: &str = concat!(
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    r#"<Relationship Id="rId1" Target="xl/workbook.xml" Type="#,
    r#""http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"/>"#,
    "</Relationships>"
);

const NO_RELS: &str =
    r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#;

const NO_TYPES: &str =
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#;

/// How many parts the archive has besides the content types.
const PARTS: u32 = 5;

/// A package of stored parts, in which the part that `slot` picks among the
/// first three is `xml`.
fn package(slot: u8, xml: &[u8]) -> Option<Vec<u8>> {
    let mut parts: [(&str, &[u8]); 6] = [
        ("_rels/.rels", PACKAGE_RELS.as_bytes()),
        ("xl/_rels/workbook.xml.rels", NO_RELS.as_bytes()),
        ("[Content_Types].xml", NO_TYPES.as_bytes()),
        ("xl/workbook.xml", b"<workbook/>"),
        ("xl/vbaProject.bin", b"vba"),
        ("xl/macrosheets/sheet1.xml", b"<macrosheet/>"),
    ];
    parts[usize::from(slot) % 3].1 = xml;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in parts {
        writer.start_file(name, options).ok()?;
        writer.write_all(content).ok()?;
    }
    Some(writer.finish().ok()?.into_inner())
}

fuzz_target!(|input: (u8, &[u8])| {
    let (slot, xml) = input;
    let Some(bytes) = package(slot, xml) else {
        return;
    };
    match read_ooxml(&bytes) {
        // A part counts once, however many entries of the XML name it.
        Ok(package) => assert!(package.macro_sheets <= PARTS),
        Err(error) => {
            // The archive is sound, so the refusal comes from the XML. A `zip.`
            // code would mean that this target has stopped building archives.
            assert!(!error.code.starts_with("zip."), "{}", error.code);
            assert_typed_error(None, error.status, error.code, &error.message);
        }
    }
});
