// SPDX-License-Identifier: Apache-2.0
//! What the content types of a package say about its parts: which of them are
//! VBA projects and which are Excel 4.0 macro sheets.
//!
//! `[Content_Types].xml` gives a content type to one part by name, with an
//! `Override`, or to every part with an extension, with a `Default`, and the
//! override wins. Only the parts of the archive are kept, so that what the
//! reader holds weighs what the archive's directory does, never what a hostile
//! `[Content_Types].xml` of 16 MiB could list.
//!
//! The media type is compared without regard to case, to the parameters after a
//! `;` or to the spaces around it. That errs on the side of finding a part that
//! Excel may ignore: every macro sheet counts, and a VBA project found by its
//! type is one that no relationship led to.

use std::collections::BTreeMap;

use super::MAX_XML_PART;
use super::archive::Archive;
use super::xml::{AppliesTo, content_types};
use crate::container::ContainerError;

/// The part that holds the content types. It is not a part of the package, and
/// has no content type.
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";

/// The content type of a VBA project.
const VBA_PROJECT_TYPE: &str = "application/vnd.ms-office.vbaProject";

/// The content types of an Excel 4.0 macro sheet: in XML, and in a binary
/// workbook (SPEC M1).
const MACRO_SHEET_XML: &str = "application/vnd.ms-excel.macrosheet+xml";
const MACRO_SHEET_BINARY: &str = "application/vnd.ms-excel.macrosheet";

/// What a content type makes of a part, as far as the reader cares.
#[derive(Clone, Copy, Default)]
pub(super) struct PartKinds {
    pub(super) vba_project: bool,
    pub(super) macro_sheet: bool,
}

impl PartKinds {
    fn of(content_type: &str) -> Self {
        let media_type = content_type.split(';').next().unwrap_or_default().trim();
        let is = |expected: &str| media_type.eq_ignore_ascii_case(expected);
        Self {
            vba_project: is(VBA_PROJECT_TYPE),
            macro_sheet: is(MACRO_SHEET_XML) || is(MACRO_SHEET_BINARY),
        }
    }

    /// What two content types make of one part, when a part is given both.
    fn merge(self, other: Self) -> Self {
        Self {
            vba_project: self.vba_project || other.vba_project,
            macro_sheet: self.macro_sheet || other.macro_sheet,
        }
    }
}

/// The extension of a part name: what follows the last dot of its last
/// segment.
fn extension_of(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next()?;
    let (_, extension) = file.rsplit_once('.')?;
    Some(extension).filter(|extension| !extension.is_empty())
}

/// What the content types say about the parts of an archive. A part is an entry
/// of the archive that is neither a directory nor the content types part, and
/// is known by its canonical name, as [`Archive::canonical`] gives it.
pub(super) struct ContentTypes<'a> {
    /// For each extension that a part has, what the `Default`s of that
    /// extension say.
    by_extension: BTreeMap<&'a str, PartKinds>,
    /// Every part, with what the `Override`s of that part say, if there are
    /// any.
    by_part: BTreeMap<&'a str, Option<PartKinds>>,
}

impl<'a> ContentTypes<'a> {
    /// Reads the content types of `archive`. A package that has none is read as
    /// if it gave no part a type. A part may be given several types, which is
    /// not valid: it then is of all of them.
    pub(super) fn read(archive: &'a Archive<'_>) -> Result<Self, ContainerError> {
        let mut types = Self {
            by_extension: BTreeMap::new(),
            by_part: BTreeMap::new(),
        };
        // A directory entry has no content and is no part.
        for name in archive.names().filter(|name| !name.ends_with('/')) {
            types.by_part.insert(name, None);
            if let Some(extension) = extension_of(name) {
                types.by_extension.entry(extension).or_default();
            }
        }
        if let Some(own) = archive.canonical(CONTENT_TYPES_PART) {
            types.by_part.remove(own);
        }
        let Some(xml) = archive.read(CONTENT_TYPES_PART, MAX_XML_PART)? else {
            return Ok(types);
        };
        content_types(&xml, |applies_to, content_type| {
            let kinds = PartKinds::of(content_type);
            match applies_to {
                AppliesTo::Extension(extension) => {
                    let slot = types
                        .by_extension
                        .get_mut(extension.to_ascii_lowercase().as_str());
                    if let Some(slot) = slot {
                        *slot = slot.merge(kinds);
                    }
                }
                AppliesTo::Part(part) => {
                    let name = archive.canonical(part);
                    if let Some(slot) = name.and_then(|name| types.by_part.get_mut(name)) {
                        *slot = Some(slot.unwrap_or_default().merge(kinds));
                    }
                }
            }
        })?;
        Ok(types)
    }

    /// Whether `canonical`, a name as [`Archive::canonical`] gives it, is that
    /// of a part. A name as written in a relationship is not: it has to be made
    /// canonical first, and no directory or content types part is a part.
    pub(super) fn is_part(&self, canonical: &str) -> bool {
        self.by_part.contains_key(canonical)
    }

    /// The parts of the archive, in order of name, with what the content types
    /// say about each: its override, else the default of its extension.
    pub(super) fn parts(&self) -> impl Iterator<Item = (&'a str, PartKinds)> + '_ {
        self.by_part.iter().map(|(name, kinds)| {
            let default = extension_of(name).and_then(|extension| self.by_extension.get(extension));
            (*name, kinds.or(default.copied()).unwrap_or_default())
        })
    }
}
