// SPDX-License-Identifier: Apache-2.0
//! The XML parts of an OPC package that the reader needs: the relationships.
//!
//! A pull parser reads them. What the reader keeps of a part is the target of
//! each relationship of a role it needs, a copy of some of the part's own text,
//! so it never weighs more than the part, which the cap on XML parts bounds.
//! Namespaces are ignored, a DTD is refused, and a part must be UTF-8, as
//! ADR-0002 has it.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use crate::container::ContainerError;
use crate::report::Status;

/// Deepest nesting of elements accepted. A relationships part is two levels
/// deep. The parser keeps the name and the offset of each open element, so a
/// part of the largest size that does nothing but open elements would cost
/// several times its size.
const MAX_DEPTH: usize = 32;

/// The type of the relationship from the package to its workbook, and the same
/// in the Strict variant of the format.
const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const OFFICE_DOCUMENT_STRICT: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";

/// The type of the relationship from a workbook to its VBA project.
const VBA_PROJECT: &str = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";

/// What a relationship says its target is, as far as the reader cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    /// The main part of the package.
    Workbook,
    /// A VBA project.
    VbaProject,
}

impl Role {
    /// The role a relationship type stands for, without regard to case. The
    /// whole URI counts, not its last segment, so that a relationship of an
    /// unrelated type cannot take the place of the real one.
    fn of(relationship_type: &str) -> Option<Self> {
        let is = |uri: &str| relationship_type.eq_ignore_ascii_case(uri);
        if is(OFFICE_DOCUMENT) || is(OFFICE_DOCUMENT_STRICT) {
            Some(Self::Workbook)
        } else if is(VBA_PROJECT) {
            Some(Self::VbaProject)
        } else {
            None
        }
    }
}

/// An internal relationship of a role the reader cares about.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Relationship {
    pub(super) role: Role,
    /// As written in the part: relative to the part that holds the
    /// relationship, or absolute.
    pub(super) target: String,
}

/// The relationships of a `.rels` part.
pub(super) fn relationships(xml: &[u8]) -> Result<Vec<Relationship>, ContainerError> {
    let mut kept = Vec::new();
    children(xml, "Relationships", |element| {
        if element.local_name().as_ref() != "Relationship" {
            return Ok(());
        }
        let role = attribute(element, "Type")?.and_then(|kind| Role::of(&kind));
        let Some(role) = role else {
            return Ok(());
        };
        let mode = attribute(element, "TargetMode")?;
        let external = mode.is_some_and(|mode| mode.eq_ignore_ascii_case("External"));
        if let (false, Some(target)) = (external, attribute(element, "Target")?) {
            kept.push(Relationship { role, target });
        }
        Ok(())
    })?;
    Ok(kept)
}

fn malformed(message: &'static str) -> ContainerError {
    ContainerError::new(Status::Corrupted, "ooxml.malformed_xml", message)
}

/// The value of the attribute `name` of an element. An element that gives it
/// twice is not well formed.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, ContainerError> {
    let bad = || malformed("an attribute of an XML part is not well formed");
    let mut value = None;
    // The crate checks all the attributes of an element against one another,
    // which takes time in proportion to the square of their number.
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|_| bad())?;
        if attribute.key.as_ref() != name {
            continue;
        }
        let text = attribute.normalized_value(XmlVersion::Implicit1_0);
        let text = text.map_err(|_| bad())?.into_owned();
        if value.replace(text).is_some() {
            return Err(malformed("an attribute of an XML part is repeated"));
        }
    }
    Ok(value)
}

/// Calls `visit` on each child of the root element, which must be `root`.
fn children(
    xml: &[u8],
    root: &str,
    mut visit: impl FnMut(&BytesStart<'_>) -> Result<(), ContainerError>,
) -> Result<(), ContainerError> {
    let text = std::str::from_utf8(xml).map_err(|_| malformed("an XML part is not UTF-8"))?;
    let mut reader = Reader::from_str(text);
    let (mut depth, mut closed) = (0_usize, false);
    loop {
        let (element, opens) = match reader.read_event() {
            Ok(Event::Start(element)) => (element, true),
            Ok(Event::Empty(element)) => (element, false),
            Ok(Event::End(_)) => {
                // The parser refuses an end tag that closes no element, so
                // `depth` is never zero here: the subtraction cannot wrap.
                depth = depth.saturating_sub(1);
                closed |= depth == 0;
                continue;
            }
            Ok(Event::DocType(_)) => return Err(malformed("an XML part has a DTD")),
            Ok(Event::Eof) => break,
            Ok(_) => continue,
            Err(_) => return Err(malformed("an XML part is not well formed")),
        };
        if depth == 0 {
            if closed || element.local_name().as_ref() != root {
                return Err(malformed("an XML part has an unexpected root"));
            }
            closed = !opens;
        } else if depth == 1 {
            visit(&element)?;
        }
        if opens {
            depth = depth.saturating_add(1);
            if depth > MAX_DEPTH {
                return Err(malformed("an XML part is nested too deeply"));
            }
        }
    }
    if depth != 0 || !closed {
        return Err(malformed("an XML part is cut short"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relationships_keep_the_roles_that_matter() {
        let xml = r#"<Relationships xmlns="x">
            <Relationship Id="1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
            <Relationship Id="2" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="/xl/strict.xml"/>
            <Relationship Id="3" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/>
            <Relationship Id="4" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProjectSignature" Target="sig.bin"/>
            <Relationship Id="5" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="http://x/y" TargetMode="External"/>
            <Relationship Id="6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme.xml"/>
            <Relationship Id="7" Type="http://example.com/other/vbaProject" Target="decoy.bin"/>
            <Relationship Id="8" Type="http://example.com/other/officeDocument" Target="decoy.xml"/>
            <Relationship Id="9" Type="HTTP://schemas.microsoft.com/office/2006/relationships/VBAPROJECT" Target="upper.bin"/>
            <Relationship Id="10" Target="untyped.bin"/>
            </Relationships>"#;
        let found = relationships(xml.as_bytes()).unwrap();
        let summary: Vec<_> = found.iter().map(|r| (r.role, r.target.as_str())).collect();
        assert_eq!(
            summary,
            [
                (Role::Workbook, "xl/workbook.xml"),
                (Role::Workbook, "/xl/strict.xml"),
                (Role::VbaProject, "vbaProject.bin"),
                (Role::VbaProject, "upper.bin"),
            ]
        );
    }

    #[test]
    fn a_part_is_read_as_utf8_with_or_without_a_byte_order_mark() {
        let xml = r#"<Relationships><Relationship Id="1" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="a.bin"/></Relationships>"#;
        let expected = [Relationship {
            role: Role::VbaProject,
            target: "a.bin".into(),
        }];
        assert_eq!(relationships(xml.as_bytes()).unwrap(), expected);
        let mut marked = vec![0xef, 0xbb, 0xbf];
        marked.extend_from_slice(xml.as_bytes());
        assert_eq!(relationships(&marked).unwrap(), expected);
        // Another encoding is not read: UTF-16 is not UTF-8.
        let units = xml.encode_utf16().flat_map(u16::to_le_bytes);
        let utf16: Vec<u8> = [0xff, 0xfe].into_iter().chain(units).collect();
        let error = relationships(&utf16).unwrap_err();
        let found = (error.status, error.code);
        assert_eq!(found, (Status::Corrupted, "ooxml.malformed_xml"));
    }

    #[test]
    fn parts_that_are_not_well_formed_are_corrupted() {
        let vba = "http://schemas.microsoft.com/office/2006/relationships/vbaProject";
        let target_twice = format!(
            r#"<Relationships><Relationship Type="{vba}" Target="a" Target="b"/></Relationships>"#
        );
        let type_twice = r#"<Relationships><Relationship Type="x" Type="y"/></Relationships>"#;
        let cases = [
            "",
            "<Relationships>",
            "<Relationships></Relationships><Relationships/>",
            "</Relationships>",
            "<Relationships/></Relationships>",
            "<Other/>",
            "<Relationships><Relationship></Relationships>",
            "<Relationships><Relationship Target=a/></Relationships>",
            "<!DOCTYPE Relationships []><Relationships/>",
            "<Relationships>\u{0}</Relationships",
            target_twice.as_str(),
            type_twice,
        ];
        for xml in cases {
            let error = relationships(xml.as_bytes()).unwrap_err();
            let found = (error.status, error.code);
            assert_eq!(found, (Status::Corrupted, "ooxml.malformed_xml"), "{xml}");
        }
        assert!(relationships(&[0xc3, 0x28]).is_err());
        assert!(relationships(b"<Relationships/>").is_ok());
        assert!(relationships(b"<Relationships></Relationships>").is_ok());
    }

    #[test]
    fn deep_parts_are_bounded() {
        let nested = |depth: usize| {
            let (open, close) = ("<a>".repeat(depth), "</a>".repeat(depth));
            format!("<Relationships>{open}{close}</Relationships>")
        };
        assert!(relationships(nested(MAX_DEPTH).as_bytes()).is_err());
        assert!(relationships(nested(MAX_DEPTH - 1).as_bytes()).is_ok());
    }
}
