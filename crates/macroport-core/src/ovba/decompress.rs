// SPDX-License-Identifier: Apache-2.0
//! MS-OVBA compression (section 2.4.1).
//!
//! A compressed container is a signature byte followed by chunks. A chunk
//! decompresses to at most 4096 bytes. It holds them raw, or as tokens: a
//! literal byte, or a copy token that points back into the same chunk.

use super::OvbaError;
use crate::report::Status;

/// First byte of a compressed container.
const CONTAINER_SIGNATURE: u8 = 0x01;
/// The bits of a chunk header that hold its signature (bits 12 to 14) and
/// their value.
const CHUNK_SIGNATURE_MASK: u16 = 0x7000;
const CHUNK_SIGNATURE: u16 = 0x3000;
/// Chunk header flag: the chunk holds tokens rather than raw bytes.
const COMPRESSED_FLAG: u16 = 0x8000;
/// The part of a chunk header that gives the size of the chunk.
const CHUNK_SIZE_MASK: u16 = 0x0FFF;
/// The size field of a raw chunk, which holds 4096 bytes.
const RAW_CHUNK_SIZE: usize = 4095;
/// The most bytes that a chunk decompresses to.
const CHUNK_OUTPUT: usize = 4096;
/// The fewest bits that a copy token gives to the offset.
const MIN_OFFSET_BITS: u32 = 4;
/// The bits of a flag byte, one per token of the sequence it heads.
const FLAG_BITS: [u8; 8] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80];

/// Decompresses a compressed container.
///
/// A container is read whole: nothing of a broken one is returned. A chunk
/// whose header declares more bytes than the container holds ends with the
/// container, as section 2.4.1.3.2 reads it, unless it is a raw chunk, which
/// needs all of its 4096 bytes. No allocation follows a size that the input
/// declares; the output grows with the bytes it holds, a chunk at a time, so
/// it overshoots `max_output` by about a chunk at most before the error.
///
/// What section 2.4.1.1 asks of writers alone is not enforced, as the reading
/// algorithm of section 2.4.1.3 does not: a chunk other than the last may
/// decompress to fewer than 4096 bytes, and a token sequence may hold no
/// token.
///
/// # Errors
///
/// An error if the container breaks a rule of section 2.4.1, or decompresses
/// to more than `max_output` bytes. It says how many bytes had been
/// decompressed by then ([`OvbaError::produced`]): a container can break after
/// `max_output` bytes, and the work is done all the same.
pub fn decompress(input: &[u8], max_output: usize) -> Result<Vec<u8>, OvbaError> {
    let mut output = Vec::new();
    match decompress_chunks(input, max_output, &mut output) {
        Ok(()) => Ok(output),
        Err(error) => Err(error.with_produced(output.len())),
    }
}

/// Appends to `output` the bytes that the chunks of the container `input`
/// decompress to, up to the one that crosses `max_output`.
fn decompress_chunks(
    input: &[u8],
    max_output: usize,
    output: &mut Vec<u8>,
) -> Result<(), OvbaError> {
    let Some((&signature, mut rest)) = input.split_first() else {
        return Err(truncated("the container is empty"));
    };
    if signature != CONTAINER_SIGNATURE {
        return Err(OvbaError::corrupted(
            "ovba.decompress.bad_signature",
            format!("the container starts with {signature:#04x}, not 0x01"),
        ));
    }
    while !rest.is_empty() {
        let (chunk, tail) = split_chunk(rest)?;
        chunk.decompress_into(output)?;
        if output.len() > max_output {
            return Err(OvbaError::new(
                Status::SuspiciousArchive,
                "ovba.decompress.too_large",
                format!("the container decompresses to more than {max_output} bytes"),
            ));
        }
        rest = tail;
    }
    Ok(())
}

/// A chunk of a container, without its header.
struct Chunk<'a> {
    compressed: bool,
    /// Whether the container ends before the chunk does: its header declares
    /// more bytes than follow.
    cut: bool,
    data: &'a [u8],
}

/// Splits the first chunk off `input`.
fn split_chunk(input: &[u8]) -> Result<(Chunk<'_>, &[u8]), OvbaError> {
    let Some((header, rest)) = input.split_first_chunk::<2>() else {
        return Err(truncated("a chunk header is cut short"));
    };
    let header = u16::from_le_bytes(*header);
    if header & CHUNK_SIGNATURE_MASK != CHUNK_SIGNATURE {
        return Err(bad_chunk("a chunk header has a wrong signature"));
    }
    let compressed = header & COMPRESSED_FLAG != 0;
    // The size field is the size of the chunk, header included, less 3.
    let size = usize::from(header & CHUNK_SIZE_MASK);
    if !compressed && size != RAW_CHUNK_SIZE {
        return Err(bad_chunk("a raw chunk is not 4096 bytes long"));
    }
    // A chunk that runs past the end of the container ends with it.
    let declared = size.saturating_add(1);
    let (data, tail) = rest.split_at_checked(declared).unwrap_or((rest, &[]));
    let cut = data.len() < declared;
    if cut && !compressed {
        return Err(truncated("a raw chunk is cut short"));
    }
    Ok((
        Chunk {
            compressed,
            cut,
            data,
        },
        tail,
    ))
}

impl Chunk<'_> {
    /// Appends the bytes that the chunk decompresses to.
    fn decompress_into(&self, output: &mut Vec<u8>) -> Result<(), OvbaError> {
        if !self.compressed {
            output.extend_from_slice(self.data);
            return Ok(());
        }
        let start = output.len();
        let mut bytes = self.data.iter().copied();
        while let Some(flags) = bytes.next() {
            for flag_bit in FLAG_BITS {
                // The last sequence of a chunk may hold fewer than 8 tokens.
                let Some(first) = bytes.next() else { break };
                if flags & flag_bit == 0 {
                    output.push(first);
                } else {
                    let Some(second) = bytes.next() else {
                        // The end of the chunk cuts the token, or that of the
                        // container if the chunk runs past it.
                        return Err(if self.cut {
                            truncated("the container ends inside a copy token")
                        } else {
                            bad_token("a copy token is cut short")
                        });
                    };
                    copy(output, start, u16::from_le_bytes([first, second]))?;
                }
                if output.len().saturating_sub(start) > CHUNK_OUTPUT {
                    return Err(bad_chunk("a chunk decompresses to more than 4096 bytes"));
                }
            }
        }
        Ok(())
    }
}

/// Appends the bytes that a copy token points at, one at a time: the source
/// may run into the bytes that the copy itself appends.
fn copy(output: &mut Vec<u8>, chunk_start: usize, token: u16) -> Result<(), OvbaError> {
    let difference = output.len().saturating_sub(chunk_start);
    if difference == 0 {
        return Err(bad_token("a chunk starts with a copy token"));
    }
    // The token splits into an offset and a length. The offset takes as many
    // bits as it needs to reach back to the start of the chunk.
    let offset_bits = offset_bits(difference);
    let length_mask = u16::MAX.checked_shr(offset_bits).unwrap_or(0);
    let length = usize::from(token & length_mask).saturating_add(3);
    let offset = usize::from(
        (token & !length_mask)
            .checked_shr(16_u32.saturating_sub(offset_bits))
            .unwrap_or(0),
    )
    .saturating_add(1);
    let Some(from) = output
        .len()
        .checked_sub(offset)
        .filter(|from| *from >= chunk_start)
    else {
        return Err(bad_token("a copy token points before its chunk"));
    };
    for index in from..from.saturating_add(length) {
        let Some(&byte) = output.get(index) else {
            return Err(bad_token("a copy token points past its chunk"));
        };
        output.push(byte);
    }
    Ok(())
}

/// The bits of a copy token that hold the offset when `difference` bytes of
/// the chunk are decompressed: those that reach back to its start, and at
/// least 4. That is the ceiling of the base 2 logarithm of `difference`.
fn offset_bits(difference: usize) -> u32 {
    let needed = usize::BITS.saturating_sub(difference.saturating_sub(1).leading_zeros());
    needed.max(MIN_OFFSET_BITS)
}

fn truncated(message: &str) -> OvbaError {
    OvbaError::corrupted("ovba.decompress.truncated", message)
}

fn bad_chunk(message: &str) -> OvbaError {
    OvbaError::corrupted("ovba.decompress.bad_chunk", message)
}

fn bad_token(message: &str) -> OvbaError {
    OvbaError::corrupted("ovba.decompress.bad_token", message)
}
