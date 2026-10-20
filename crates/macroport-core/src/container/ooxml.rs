// SPDX-License-Identifier: Apache-2.0
//! OOXML packages (`.xlsm .xltm .xlam .xlsb .xlsx .xltx`): ZIP archives from
//! which the reader takes the VBA project, within the limits of SPEC M1.

mod archive;
mod xml;

use std::fmt;

use archive::Archive;
use xml::{Relationship, Role, relationships};

use crate::container::ContainerError;

/// Largest VBA part the reader inflates (SPEC M1): 64 MiB.
const MAX_VBA_PART: u64 = 64 * 1024 * 1024;

/// Largest XML part the reader inflates (SPEC M1): 16 MiB.
const MAX_XML_PART: u64 = 16 * 1024 * 1024;

/// The part Excel stores the VBA project in.
const VBA_PART: &str = "xl/vbaProject.bin";

/// The part that holds the relationships of the package itself.
const PACKAGE_RELATIONSHIPS: &str = "_rels/.rels";

/// Longest part name that the reader follows a relationship to. A ZIP entry
/// may have a name of 65,535 bytes, and a relationship may repeat the
/// directory of its part once per target, so a bound keeps a hostile part from
/// costing more than it weighs. The longest part name of the generated fixtures
/// has 39 bytes.
const MAX_PART_NAME: usize = 1024;

/// What the reader found in a package.
pub struct OoxmlPackage {
    /// The VBA project: a compound file.
    pub vba_project: Option<Vec<u8>>,
}

/// Prints the size of the VBA project, not up to 64 MiB of it.
impl fmt::Debug for OoxmlPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = self.vba_project.as_ref().map(Vec::len);
        f.debug_struct("OoxmlPackage")
            .field("vba_project_len", &length)
            .finish()
    }
}

/// Reads an OOXML package.
///
/// The ZIP archive is read without being trusted: an entry that inflates
/// beyond its limit or its ratio makes the file a suspicious archive, however
/// small the sizes in its headers say it is. The VBA project is the part that
/// a relationship of the workbook names, else the part called
/// `xl/vbaProject.bin`.
pub fn read_ooxml(bytes: &[u8]) -> Result<OoxmlPackage, ContainerError> {
    let archive = Archive::open(bytes)?;
    let package = Relations::of_package(&archive)?;
    let workbook = match package.targets(Role::Workbook).next() {
        Some(part) => Relations::of_part(&archive, &part)?,
        None => Relations::default(),
    };
    Ok(OoxmlPackage {
        vba_project: vba_project(&archive, &workbook)?,
    })
}

/// The VBA project: the part that a relationship of the workbook names, else
/// the usual one. The workbooks that Excel wrote for the fixtures relate their
/// project to the workbook and to nothing else.
fn vba_project(
    archive: &Archive<'_>,
    workbook: &Relations,
) -> Result<Option<Vec<u8>>, ContainerError> {
    let named = workbook.targets(Role::VbaProject);
    for part in named.chain([VBA_PART.to_owned()]) {
        if let Some(project) = archive.read(&part, MAX_VBA_PART)? {
            return Ok(Some(project));
        }
    }
    Ok(None)
}

/// The relationships of one part, and the directory their targets are
/// relative to.
#[derive(Default)]
struct Relations {
    directory: String,
    relationships: Vec<Relationship>,
}

impl Relations {
    /// The relationships that the part called `rels` holds, for a part in
    /// `directory`. A package without that part has none.
    fn read(archive: &Archive<'_>, rels: &str, directory: &str) -> Result<Self, ContainerError> {
        let relationships = match archive.read(rels, MAX_XML_PART)? {
            Some(xml) => relationships(&xml)?,
            None => Vec::new(),
        };
        Ok(Self {
            directory: directory.to_owned(),
            relationships,
        })
    }

    /// The relationships of the package itself, whose targets are relative to
    /// the root.
    fn of_package(archive: &Archive<'_>) -> Result<Self, ContainerError> {
        Self::read(archive, PACKAGE_RELATIONSHIPS, "")
    }

    /// The relationships of the part called `part`, which are in the `_rels`
    /// directory next to it.
    fn of_part(archive: &Archive<'_>, part: &str) -> Result<Self, ContainerError> {
        let (directory, file) = part.rsplit_once('/').unwrap_or(("", part));
        let rels = if directory.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{directory}/_rels/{file}.rels")
        };
        Self::read(archive, &rels, directory)
    }

    /// The names of the parts that its relationships of this `role` point to,
    /// in order. Each name is made when asked for and dropped after use, so
    /// that the many relationships of a hostile part cost no more memory than
    /// the part does.
    fn targets(&self, role: Role) -> impl Iterator<Item = String> + '_ {
        let of_role = (self.relationships.iter()).filter(move |found| found.role == role);
        of_role.filter_map(|relationship| resolve(&self.directory, &relationship.target))
    }
}

/// The name of the part that a relationship `target` points to, from a part in
/// `directory`, or `None` when it leads out of the package or is longer than
/// [`MAX_PART_NAME`]. The name is not normalised: the archive does that when
/// it looks the part up. Percent-encoding is not decoded.
fn resolve(directory: &str, target: &str) -> Option<String> {
    if directory.len().saturating_add(target.len()) > MAX_PART_NAME {
        return None;
    }
    let target = target.replace('\\', "/");
    let path = target.split('#').next()?;
    let mut segments: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect()
    };
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            _ => segments.push(segment),
        }
    }
    Some(segments.join("/"))
}
