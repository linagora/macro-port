// SPDX-License-Identifier: Apache-2.0
//! Text in a VBA project: MBCS text in the code page of the project.

use encoding_rs::{UTF_16LE, WINDOWS_1252};

/// The code page of UTF-16 little-endian, which MS-OVBA does not allow for a
/// project but which the SPEC asks to handle.
const UTF16_LE_CODE_PAGE: u16 = 1200;

/// Decodes MBCS text in the code page of the project. A byte that the code
/// page does not map becomes U+FFFD, and a byte order mark is not special:
/// the bytes are in the code page that the project declares, whatever they
/// start with.
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
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}
