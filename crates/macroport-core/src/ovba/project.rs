// SPDX-License-Identifier: Apache-2.0
//! The `PROJECT` stream of a VBA project (MS-OVBA section 2.3.1): text in the
//! code page of the project, with one `Key=value` property per line. Its
//! properties come first, and the sections that follow (`[Host Extender
//! Info]`, `[Workspace]`) are of no use here.

use super::MAX_PROJECT_STREAM;
use super::encryption::decrypt;
use super::text::decode_mbcs;

/// The visibility state of a project that is not visible: a protected one.
const NOT_VISIBLE: u8 = 0x00;

/// What the `PROJECT` stream says about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Whether the project is protected: its visibility state (the `GC`
    /// property), once decrypted, is 0x00, which is not visible. A project
    /// whose `GC` is missing or cannot be read is not protected. No password
    /// is recovered, and the source of the modules is not encrypted either
    /// way.
    pub protected: bool,
}

/// Reads a `PROJECT` stream.
///
/// `code_page` is the code page of the project, which its `dir` stream gives
/// (see [`parse_dir`](super::parse_dir)). The stream is hostile like any
/// other, and a line that is not understood is skipped, so reading does not
/// fail. The key of a property is read whatever its case, the first property
/// of a kind wins, the properties end at the first line that opens a section
/// (`[`), and no more than the first [`MAX_PROJECT_STREAM`] bytes of the stream
/// are read: a longer stream is cut there, not rejected.
#[must_use]
pub fn parse_project(stream: &[u8], code_page: Option<u16>) -> ProjectInfo {
    let stream = stream.get(..MAX_PROJECT_STREAM).unwrap_or(stream);
    let text = decode_mbcs(stream, code_page);
    let mut protected = None;
    // MS-OVBA ends a line with CR LF or LF CR. Splitting at every CR and LF
    // reads both, and either alone; the empty lines that result are no
    // property.
    for line in text.split(['\r', '\n']) {
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.eq_ignore_ascii_case("GC") {
            protected.get_or_insert_with(|| says_protected(value));
        }
    }
    ProjectInfo {
        protected: protected.unwrap_or(false),
    }
}

/// Whether the value of a `GC` property, the hexadecimal digits of an Encrypted
/// Data Structure between double quotes, holds one byte that is 0x00: it says
/// that the project is protected.
fn says_protected(value: &str) -> bool {
    let digits = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'));
    let Some(structure) = digits.and_then(hex_bytes) else {
        return false;
    };
    decrypt(&structure).is_some_and(|data| data == [NOT_VISIBLE])
}

/// The bytes that hexadecimal digits write, in either case.
fn hex_bytes(digits: &str) -> Option<Vec<u8>> {
    let (pairs, odd) = digits.as_bytes().as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[high, low]| Some((nibble(high)? << 4) | nibble(low)?))
        .collect()
}

/// The value of a hexadecimal digit.
fn nibble(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}
