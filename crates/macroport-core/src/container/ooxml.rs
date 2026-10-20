// SPDX-License-Identifier: Apache-2.0
//! OOXML packages (`.xlsm .xltm .xlam .xlsb .xlsx .xltx`): ZIP archives from
//! which the reader takes the VBA project, within the limits of SPEC M1.

mod archive;

use std::fmt;

use archive::Archive;

use crate::container::ContainerError;

/// Largest VBA part the reader inflates (SPEC M1): 64 MiB.
const MAX_VBA_PART: u64 = 64 * 1024 * 1024;

/// The part Excel stores the VBA project in.
const VBA_PART: &str = "xl/vbaProject.bin";

/// What the reader found in a package.
pub struct OoxmlPackage {
    /// The VBA project: a compound file, `xl/vbaProject.bin` in the archive.
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
/// small the sizes in its headers say it is.
pub fn read_ooxml(bytes: &[u8]) -> Result<OoxmlPackage, ContainerError> {
    let archive = Archive::open(bytes)?;
    Ok(OoxmlPackage {
        vba_project: archive.read(VBA_PART, MAX_VBA_PART)?,
    })
}
