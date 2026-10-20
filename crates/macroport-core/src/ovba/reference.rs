// SPDX-License-Identifier: Apache-2.0
//! The reference records of the `dir` stream and the display name of a
//! reference.
//!
//! MS-OVBA has a reader ignore the size of a reference record, so these
//! records are read field by field and never skipped by their size. A display
//! name never holds a path: the libids and the paths of a reference may name
//! a user (SPEC M1, item 7).

use super::OvbaError;
use super::reader::Reader;
use super::text::{self, RawName};

pub(super) const NAME: u16 = 0x0016;
/// The marker that opens the Unicode half of a reference name.
const NAME_UNICODE: u16 = 0x003E;
pub(super) const REGISTERED: u16 = 0x000D;
pub(super) const PROJECT: u16 = 0x000E;
pub(super) const CONTROL: u16 = 0x002F;

/// The size field of a reference record, which a reader ignores.
const IGNORED_SIZE: usize = 4;
/// A reserved field (4 bytes) and a second one (2) that end a registered
/// reference, and the two versions (4 and 2) that end a project reference.
const TAIL: usize = 6;
/// A field of 2 bytes that holds a record id: the marker of a Unicode name,
/// the id of a name record, or the id of the extended part of a control
/// reference.
const MARKER: usize = 2;
/// What follows the extended libid of a control reference: two reserved
/// fields (4 and 2 bytes), the original type library (16) and a cookie (4).
const CONTROL_TAIL: usize = 26;

/// A reference, with what its display name can come from.
#[derive(Debug, Default)]
pub(super) struct RawReference {
    /// Type library identifiers, the most telling first. Empty for a project
    /// reference, whose identifiers are paths.
    libids: Vec<Vec<u8>>,
    /// The name that the record holds. Only a control reference has one.
    pub(super) name: RawName,
}

impl RawReference {
    /// The display name: the registered name of the first type library that
    /// has one, else the name of the reference. A name that looks like a path
    /// is passed over, as it may name a user.
    pub(super) fn display_name(&self, code_page: Option<u16>) -> Option<String> {
        self.libids
            .iter()
            .filter_map(|libid| registered_name(&text::decode_mbcs(libid, code_page)))
            .chain(self.name.text(code_page))
            .find(|name| !looks_like_a_path(name))
    }
}

/// A REFERENCENAME record, past its id: the name in MBCS and, when the marker
/// follows, the same name in Unicode.
pub(super) fn name_record(reader: &mut Reader<'_>) -> Result<RawName, OvbaError> {
    let mbcs = reader.length_prefixed()?.to_vec();
    let unicode = if reader.peek_u16() == Some(NAME_UNICODE) {
        reader.skip(MARKER)?;
        reader.length_prefixed()?.to_vec()
    } else {
        Vec::new()
    };
    Ok(RawName { mbcs, unicode })
}

/// A REFERENCEREGISTERED record, past its id: a size, the libid of a type
/// library and reserved bytes.
pub(super) fn registered(reader: &mut Reader<'_>) -> Result<RawReference, OvbaError> {
    reader.skip(IGNORED_SIZE)?;
    let libid = reader.length_prefixed()?.to_vec();
    reader.skip(TAIL)?;
    Ok(RawReference {
        libids: vec![libid],
        name: RawName::default(),
    })
}

/// A REFERENCEPROJECT record, past its id: a size, the absolute and the
/// relative path of the project, and its version. The paths are skipped, for
/// they may name a user.
pub(super) fn project(reader: &mut Reader<'_>) -> Result<RawReference, OvbaError> {
    reader.skip(IGNORED_SIZE)?;
    reader.length_prefixed()?;
    reader.length_prefixed()?;
    reader.skip(TAIL)?;
    Ok(RawReference::default())
}

/// A REFERENCECONTROL record, past its id. It holds a twiddled libid, which
/// MS-OVBA says should be a dummy, then the name of the extended type
/// library, which is optional, and its extended libid.
pub(super) fn control(reader: &mut Reader<'_>) -> Result<RawReference, OvbaError> {
    reader.skip(IGNORED_SIZE)?;
    let twiddled = reader.length_prefixed()?.to_vec();
    reader.skip(TAIL)?;
    let name = if reader.peek_u16() == Some(NAME) {
        reader.skip(MARKER)?;
        name_record(reader)?
    } else {
        RawName::default()
    };
    // The marker of the extended part, then its size.
    reader.skip(MARKER)?;
    reader.skip(IGNORED_SIZE)?;
    let extended = reader.length_prefixed()?.to_vec();
    reader.skip(CONTROL_TAIL)?;
    Ok(RawReference {
        libids: vec![extended, twiddled],
        name,
    })
}

/// The registered name of a type library, the last field of its identifier
/// `*\G{guid}#major.minor#lcid#path#name`. The path of a Macintosh library
/// has the kind `H` rather than `G`. The path holds no `#`, so the name is
/// whatever follows the fourth one.
fn registered_name(libid: &str) -> Option<String> {
    let fields = libid
        .strip_prefix("*\\G")
        .or_else(|| libid.strip_prefix("*\\H"))?;
    let name = fields.splitn(5, '#').nth(4)?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether `text` holds a path separator or starts with a drive letter.
fn looks_like_a_path(text: &str) -> bool {
    let mut characters = text.chars();
    let drive = matches!(
        (characters.next(), characters.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    );
    drive || text.contains(['\\', '/'])
}
