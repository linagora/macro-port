// SPDX-License-Identifier: Apache-2.0
//! The XML parts of an OPC package that the reader needs: the relationships.
//!
//! A pull parser reads them. What the reader keeps of a part is the target of
//! each relationship of a role it needs, a copy of some of the part's own text,
//! so it never weighs more than the part, which the cap on XML parts bounds.
//!
//! A part that is not what the format says is `corrupted`, rather than read
//! with a leniency that Excel may not share: a DTD, text outside the root, a
//! root or a child that is not in the namespace of the format, nesting that is
//! too deep, another encoding than UTF-8 (ADR-0002 leaves its status to M1), or
//! a relationship of a role the reader needs whose target mode is neither
//! `Internal` nor `External`. The names of attributes are matched with their
//! prefix.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::reader::NsReader;

use crate::container::ContainerError;
use crate::report::Status;

/// The namespace of the relationships of a package.
const RELATIONSHIPS_NAMESPACE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";

/// Deepest nesting of elements accepted. A relationships part is two levels
/// deep. The parser keeps the name and the offset of each open element, so a
/// part of the largest size that does nothing but open elements would cost
/// several times its size.
const MAX_DEPTH: usize = 32;

/// The type of the relationship from the package to its workbook, as the
/// fixtures have it. The Strict variant of the format has another URI, which no
/// fixture uses and the reader does not know.
const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

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
    /// The role a relationship type stands for. The whole URI counts, exactly
    /// as written, not its last segment, so that a relationship of another type
    /// cannot take the place of the real one.
    fn of(relationship_type: &str) -> Option<Self> {
        match relationship_type {
            OFFICE_DOCUMENT => Some(Self::Workbook),
            VBA_PROJECT => Some(Self::VbaProject),
            _ => None,
        }
    }
}

/// An internal relationship of a role the reader cares about.
#[derive(Debug)]
pub(super) struct Relationship {
    pub(super) role: Role,
    /// As written in the part: relative to the part that holds the
    /// relationship, or absolute.
    pub(super) target: String,
}

/// The relationships of a `.rels` part.
pub(super) fn relationships(xml: &[u8]) -> Result<Vec<Relationship>, ContainerError> {
    let mut kept = Vec::new();
    children(xml, RELATIONSHIPS_NAMESPACE, "Relationships", |element| {
        if element.local_name().as_ref() != "Relationship" {
            return Ok(());
        }
        let role = attribute(element, "Type")?.and_then(|kind| Role::of(&kind));
        let Some(role) = role else {
            return Ok(());
        };
        let external = match attribute(element, "TargetMode")?.as_deref() {
            None | Some("Internal") => false,
            Some("External") => true,
            Some(_) => return Err(malformed("a relationship has an unknown target mode")),
        };
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

fn text_outside() -> ContainerError {
    malformed("an XML part has text outside its root")
}

/// Whether `byte` is white space in XML.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// The value of the attribute `name` of an element. An element that gives it
/// twice is not well formed.
fn attribute(element: &BytesStart<'_>, name: &str) -> Result<Option<String>, ContainerError> {
    let bad = || malformed("an attribute of an XML part is not well formed");
    let mut value = None;
    // The crate looks for duplicates among all the attributes of an element,
    // and keeps a range and a hash for each. For an element of over a million
    // attributes that was 50 MiB more than the part itself (release build,
    // counting allocator), so only the attribute that is read is checked.
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

/// Calls `visit` on each child of the root element. The root must be `root`,
/// and it and its children must be in `namespace`.
fn children(
    xml: &[u8],
    namespace: &str,
    root: &str,
    mut visit: impl FnMut(&BytesStart<'_>) -> Result<(), ContainerError>,
) -> Result<(), ContainerError> {
    let text = std::str::from_utf8(xml).map_err(|_| malformed("an XML part is not UTF-8"))?;
    let mut reader = NsReader::from_str(text);
    let (mut depth, mut closed) = (0_usize, false);
    loop {
        let (resolved, event) = reader
            .read_resolved_event()
            .map_err(|_| malformed("an XML part is not well formed"))?;
        let in_namespace = match resolved {
            ResolveResult::Bound(Namespace(found)) => found == namespace,
            _ => false,
        };
        let (element, opens) = match event {
            Event::Start(element) => (element, true),
            Event::Empty(element) => (element, false),
            Event::End(_) => {
                // The parser refuses an end tag that closes no element, so
                // `depth` is never zero here: the subtraction cannot wrap.
                depth = depth.saturating_sub(1);
                closed |= depth == 0;
                continue;
            }
            Event::DocType(_) => return Err(malformed("an XML part has a DTD")),
            Event::Eof => break,
            Event::Text(text) if depth == 0 && !text.bytes().all(is_space) => {
                return Err(text_outside());
            }
            Event::CData(_) | Event::GeneralRef(_) if depth == 0 => return Err(text_outside()),
            _ => continue,
        };
        if depth == 0 {
            if closed || !in_namespace || element.local_name().as_ref() != root {
                return Err(malformed("an XML part has an unexpected root"));
            }
            closed = !opens;
        } else if depth == 1 {
            if !in_namespace {
                return Err(malformed("an XML part has an element of another namespace"));
            }
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
