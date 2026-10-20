// SPDX-License-Identifier: Apache-2.0
//! Bounded access to the entries of a ZIP archive held in memory.
//!
//! The `zip` crate reads the central directory. The data of an entry is
//! inflated here instead: SPEC M1 limits the bytes actually inflated against
//! the bytes actually read, never the sizes an entry declares, and the crate's
//! own reader checks neither.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use flate2::Crc;
use flate2::bufread::DeflateDecoder;
use zip::read::{ArchiveOffset, Config, ZipArchiveMetadata};
use zip::result::ZipError;
use zip::{CompressionMethod, ZipArchive};

use crate::container::ContainerError;
use crate::report::Status;

/// Largest ratio between what an entry inflates to and the compressed bytes
/// read for it (SPEC M1). Deflate itself cannot go beyond about 1032.
const MAX_RATIO: u64 = 1000;

/// Bytes inflated per read.
const CHUNK: usize = 32 * 1024;

/// The fixed part of a local file header (APPNOTE 4.3.7): the name and the
/// extra field follow.
const LOCAL_HEADER: usize = 30;

/// A fixed message only: nothing from the archive reaches an error, as
/// everything in an archive is untrusted.
fn fail(status: Status, code: &'static str, message: &'static str) -> ContainerError {
    ContainerError::new(status, code, message)
}

fn invalid_archive() -> ContainerError {
    let message = "the ZIP archive is invalid";
    fail(Status::Corrupted, "zip.invalid_archive", message)
}

/// Excel and the analyser could pick different entries of the same name.
fn duplicate_entry() -> ContainerError {
    let message = "two entries of the archive have the same name";
    fail(Status::Corrupted, "zip.duplicate_entry", message)
}

/// The entries of a ZIP archive, by name.
pub(super) struct Archive<'a> {
    bytes: &'a [u8],
    entries: Arc<ZipArchiveMetadata>,
    /// The index of the entry each normalised name stands for.
    names: BTreeMap<String, usize>,
}

impl<'a> Archive<'a> {
    /// Reads the central directory of `bytes`.
    pub(super) fn open(bytes: &'a [u8]) -> Result<Self, ContainerError> {
        let signature = matches!(
            bytes.get(..4),
            Some([b'P', b'K', 3, 4] | [b'P', b'K', 5, 6] | [b'P', b'K', 7, 8])
        );
        if !signature {
            let message = "the file does not start with a ZIP signature";
            return Err(fail(Status::Unsupported, "zip.not_a_zip", message));
        }
        // An Office file starts with its archive. With this offset the crate
        // does not look for the directory in the whole file, which takes long
        // on a large one.
        let config = Config {
            archive_offset: ArchiveOffset::Known(0),
        };
        let entries = ZipArchive::with_config(config, Cursor::new(bytes))
            .map_err(|error| open_error(&error))?
            .metadata();
        let mut names = BTreeMap::new();
        for index in 0..entries.len() {
            let name = entry_name(&entries, index);
            if name.is_some_and(|name| names.insert(name, index).is_some()) {
                return Err(duplicate_entry());
            }
        }
        Ok(Self {
            bytes,
            entries,
            names,
        })
    }

    /// The content of the entry called `name`, or `None` when there is no such
    /// entry. At most `cap` bytes are inflated.
    pub(super) fn read(&self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, ContainerError> {
        let Some(&index) = self.names.get(&normalize(name)) else {
            return Ok(None);
        };
        let entry = self.entries.entry(index).map_err(|_| invalid_archive())?;
        if entry.encrypted() {
            let message = "an entry is encrypted";
            return Err(fail(Status::Encrypted, "zip.encrypted_entry", message));
        }
        let start = data_start(self.bytes, entry.header_start()).ok_or_else(invalid_archive)?;
        let data = usize::try_from(entry.compressed_size())
            .ok()
            .and_then(|length| start.checked_add(length))
            .and_then(|end| self.bytes.get(start..end))
            .ok_or_else(|| {
                let message = "the data of an entry lies beyond the end of the file";
                fail(Status::Corrupted, "zip.entry_out_of_bounds", message)
            })?;
        let content = match entry.compression() {
            CompressionMethod::Stored => stored(data, cap)?,
            CompressionMethod::Deflated => inflate(data, cap)?,
            _ => {
                let message = "an entry uses an unsupported compression method";
                return Err(fail(Status::Unsupported, "zip.unsupported_method", message));
            }
        };
        let mut checksum = Crc::new();
        checksum.update(&content);
        if checksum.sum() != entry.crc32() {
            let message = "the data of an entry does not match its checksum";
            return Err(fail(Status::Corrupted, "zip.crc_mismatch", message));
        }
        Ok(Some(content))
    }
}

/// The name an entry is looked up by: slashes, not backslashes, no leading
/// slash, and lowercase, as OPC compares part names without regard to case.
fn normalize(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

fn entry_name(entries: &ZipArchiveMetadata, index: usize) -> Option<String> {
    let entry = entries.entry(index).ok()?;
    let name = entry.name().ok()?;
    Some(normalize(&name))
}

/// The little-endian 16-bit field of a header at `at`.
fn u16_at(header: &[u8], at: usize) -> Option<usize> {
    let field = header.get(at..at.checked_add(2)?)?;
    Some(usize::from(u16::from_le_bytes(field.try_into().ok()?)))
}

/// Where the data of an entry starts: after its local header, its name and
/// its extra field.
fn data_start(bytes: &[u8], header_start: u64) -> Option<usize> {
    let start = usize::try_from(header_start).ok()?;
    let end = start.checked_add(LOCAL_HEADER)?;
    let header = bytes.get(start..end)?;
    if !header.starts_with(b"PK\x03\x04") {
        return None;
    }
    end.checked_add(u16_at(header, 26)?)?
        .checked_add(u16_at(header, 28)?)
}

fn open_error(error: &ZipError) -> ContainerError {
    match error {
        ZipError::UnsupportedArchive(_) => {
            let message = "the archive uses a ZIP feature that is not supported";
            fail(Status::Unsupported, "zip.unsupported", message)
        }
        _ => invalid_archive(),
    }
}

fn too_large() -> ContainerError {
    let message = "an entry is larger than its limit";
    fail(Status::SuspiciousArchive, "zip.entry_too_large", message)
}

fn allocation_failed() -> ContainerError {
    let message = "the memory for an entry is not available";
    fail(Status::TooLarge, "zip.allocation_failed", message)
}

/// Adds `chunk` to `content`, unless `content` would go beyond `cap` bytes.
fn append(content: &mut Vec<u8>, chunk: &[u8], cap: u64) -> Result<(), ContainerError> {
    let total = content.len().checked_add(chunk.len());
    if total
        .and_then(|total| u64::try_from(total).ok())
        .is_none_or(|total| total > cap)
    {
        return Err(too_large());
    }
    content
        .try_reserve(chunk.len())
        .map_err(|_| allocation_failed())?;
    content.extend_from_slice(chunk);
    Ok(())
}

/// The data of a stored entry.
fn stored(data: &[u8], cap: u64) -> Result<Vec<u8>, ContainerError> {
    let mut content = Vec::new();
    append(&mut content, data, cap)?;
    Ok(content)
}

/// Inflates a raw deflate stream: at most `cap` bytes, and at most
/// [`MAX_RATIO`] times the compressed bytes consumed so far, which is checked
/// after every chunk, so a bomb stops long before the cap.
fn inflate(compressed: &[u8], cap: u64) -> Result<Vec<u8>, ContainerError> {
    let mut decoder = DeflateDecoder::new(compressed);
    let mut content = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => return Ok(content),
            Ok(read) => append(&mut content, chunk.get(..read).unwrap_or_default(), cap)?,
            Err(_) => {
                let message = "the data of an entry is not a valid deflate stream";
                return Err(fail(Status::Corrupted, "zip.inflate", message));
            }
        }
        let inflated = u64::try_from(content.len()).unwrap_or(u64::MAX);
        if inflated > MAX_RATIO.saturating_mul(decoder.total_in()) {
            let message = "an entry inflates beyond the allowed ratio";
            return Err(fail(Status::SuspiciousArchive, "zip.ratio", message));
        }
    }
}
