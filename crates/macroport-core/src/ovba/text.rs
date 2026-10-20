// SPDX-License-Identifier: Apache-2.0
//! Text in a VBA project: UTF-16 names and MBCS text in the project code page.

use encoding_rs::{Encoding, UTF_16LE, WINDOWS_1252};

/// The code page of UTF-16 little-endian, which MS-OVBA does not allow for a
/// project but which the SPEC asks to handle.
const UTF16_LE_CODE_PAGE: u16 = 1200;

/// A name as the `dir` stream gives it, before the code page is known to
/// apply: in MBCS and, when the stream has it, in UTF-16.
#[derive(Debug, Default)]
pub(super) struct RawName {
    pub(super) mbcs: Vec<u8>,
    pub(super) unicode: Vec<u8>,
}

impl RawName {
    pub(super) fn is_empty(&self) -> bool {
        self.mbcs.is_empty() && self.unicode.is_empty()
    }

    /// The name as text, if it is not empty: the Unicode form when there is
    /// one, else the MBCS form decoded in the code page.
    pub(super) fn text(&self, code_page: Option<u16>) -> Option<String> {
        let unicode = decode_utf16le(&self.unicode);
        if !unicode.is_empty() {
            return Some(unicode);
        }
        let mbcs = decode_mbcs(&self.mbcs, code_page);
        (!mbcs.is_empty()).then_some(mbcs)
    }
}

/// Decodes UTF-16 little-endian text. A byte left over and an unpaired
/// surrogate become U+FFFD.
fn decode_utf16le(bytes: &[u8]) -> String {
    decode(UTF_16LE, bytes)
}

/// Decodes MBCS text in the code page of the project. A byte that the code
/// page does not map becomes U+FFFD.
///
/// Code page 1200 is UTF-16 little-endian. A code page that is missing or
/// that no encoding matches falls back to windows-1252, which gives every
/// byte a character and so loses none of them.
pub(super) fn decode_mbcs(bytes: &[u8], code_page: Option<u16>) -> String {
    let encoding = match code_page {
        Some(UTF16_LE_CODE_PAGE) => UTF_16LE,
        Some(code_page) => codepage::to_encoding_no_replacement(code_page).unwrap_or(WINDOWS_1252),
        None => WINDOWS_1252,
    };
    decode(encoding, bytes)
}

/// Decodes `bytes` in `encoding`, ignoring any byte order mark: the bytes
/// are in the encoding that the project declares, whatever they start with.
fn decode(encoding: &'static Encoding, bytes: &[u8]) -> String {
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}
