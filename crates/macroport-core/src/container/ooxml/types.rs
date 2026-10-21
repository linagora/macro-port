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
use super::xml::{Applies, content_types};
use crate::container::ContainerError;

/// The part that holds the content types. It is not a part of the package, and
/// has no content type.
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";

/// The content type of a VBA project.
const VBA_PROJECT: &str = "application/vnd.ms-office.vbaProject";

/// The content types of an Excel 4.0 macro sheet: in XML, and in a binary
/// workbook (SPEC M1).
const MACRO_SHEET_XML: &str = "application/vnd.ms-excel.macrosheet+xml";
const MACRO_SHEET_BINARY: &str = "application/vnd.ms-excel.macrosheet";

/// What a content type makes of a part, as far as the reader cares.
#[derive(Clone, Copy, Default)]
pub(super) struct Kinds {
    pub(super) vba_project: bool,
    pub(super) macro_sheet: bool,
}

impl Kinds {
    fn of(content_type: &str) -> Self {
        let media_type = content_type.split(';').next().unwrap_or_default().trim();
        let is = |expected: &str| media_type.eq_ignore_ascii_case(expected);
        Self {
            vba_project: is(VBA_PROJECT),
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
fn extension(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next()?;
    let (_, extension) = file.rsplit_once('.')?;
    Some(extension).filter(|extension| !extension.is_empty())
}

/// What the content types say about the parts of an archive.
pub(super) struct ContentTypes<'a> {
    /// For each extension that a part has, what the `Default`s of that
    /// extension say.
    defaults: BTreeMap<&'a str, Kinds>,
    /// For each part, what the `Override`s of that part say, if there are any.
    overrides: BTreeMap<&'a str, Option<Kinds>>,
}

impl<'a> ContentTypes<'a> {
    /// Reads the content types of `archive`. A package that has none is read as
    /// if it gave no part a type. A part may be given several types, which is
    /// not valid: it then is of all of them.
    pub(super) fn read(archive: &'a Archive<'_>) -> Result<Self, ContainerError> {
        let mut types = Self {
            defaults: BTreeMap::new(),
            overrides: BTreeMap::new(),
        };
        // A directory entry has no content and is no part.
        for name in archive.names().filter(|name| !name.ends_with('/')) {
            types.overrides.insert(name, None);
            if let Some(extension) = extension(name) {
                types.defaults.entry(extension).or_default();
            }
        }
        if let Some(own) = archive.canonical(CONTENT_TYPES_PART) {
            types.overrides.remove(own);
        }
        let Some(xml) = archive.read(CONTENT_TYPES_PART, MAX_XML_PART)? else {
            return Ok(types);
        };
        content_types(&xml, |applies, content_type| {
            let kinds = Kinds::of(content_type);
            match applies {
                Applies::Extension(extension) => {
                    let slot = types
                        .defaults
                        .get_mut(extension.to_ascii_lowercase().as_str());
                    if let Some(slot) = slot {
                        *slot = slot.merge(kinds);
                    }
                }
                Applies::Part(part) => {
                    let name = archive.canonical(part);
                    if let Some(slot) = name.and_then(|name| types.overrides.get_mut(name)) {
                        *slot = Some(slot.unwrap_or_default().merge(kinds));
                    }
                }
            }
        })?;
        Ok(types)
    }

    /// Whether the archive has a part called `name`, which is normalised: any
    /// entry but the content types.
    pub(super) fn is_part(&self, name: &str) -> bool {
        self.overrides.contains_key(name)
    }

    /// The parts of the archive, in order of name, with what the content types
    /// say about each: its override, else the default of its extension.
    pub(super) fn parts(&self) -> impl Iterator<Item = (&'a str, Kinds)> + '_ {
        self.overrides.iter().map(|(name, kinds)| {
            let default = extension(name).and_then(|extension| self.defaults.get(extension));
            (*name, kinds.or(default.copied()).unwrap_or_default())
        })
    }
}
