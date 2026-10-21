// SPDX-License-Identifier: Apache-2.0
//! The source of a module, in its Module Stream (MS-OVBA section 2.2.5).

use super::OvbaError;
use super::decompress::decompress;
use super::text::decode_text;

/// The source of a module, from the bytes of its stream: the text that
/// follows the compiled code, decoded as [`decode_text`] does, into UTF-8 with
/// `\n` line endings.
///
/// A module stream opens with the performance cache, the compiled code of the
/// module, which is `text_offset` bytes long (`MODULEOFFSET` in the `dir`
/// stream) and which is not read. The source follows it in a compressed
/// container that runs to the end of the stream. `code_page` is that of the
/// project.
///
/// `max_source` bounds the bytes that the source decompresses to, as
/// `max_output` does for [`decompress`]. The text takes up to three times as
/// many bytes, and while it is made the bytes and two copies of the text are
/// held: up to seven times `max_source` at the peak, three times for ASCII.
/// The caller that reads many modules keeps its own total.
///
/// A stream that ends at `text_offset` holds no source: the result is empty.
///
/// # Errors
///
/// An error if `text_offset` is past the end of the stream (code
/// `ovba.module.bad_offset`), or if the container breaks a rule of
/// [`decompress`], whose errors this function passes on.
pub fn module_source(
    stream: &[u8],
    text_offset: u32,
    code_page: Option<u16>,
    max_source: usize,
) -> Result<String, OvbaError> {
    let compressed = usize::try_from(text_offset)
        .ok()
        .and_then(|offset| stream.get(offset..))
        .ok_or_else(|| {
            OvbaError::corrupted(
                "ovba.module.bad_offset",
                format!(
                    "the source starts at byte {text_offset} of a stream of {} bytes",
                    stream.len()
                ),
            )
        })?;
    if compressed.is_empty() {
        return Ok(String::new());
    }
    let source = decompress(compressed, max_source)?;
    Ok(decode_text(&source, code_page))
}
