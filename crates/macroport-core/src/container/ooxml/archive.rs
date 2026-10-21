// SPDX-License-Identifier: Apache-2.0
//! Bounded access to the entries of a ZIP archive held in memory.
//!
//! The `zip` crate reads the central directory. The data of an entry is
//! inflated here instead: SPEC M1 limits the bytes actually inflated against
//! the bytes actually read, never the sizes an entry declares, and the crate's
//! own reader checks neither.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
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

/// What opening an archive may read. The `zip` crate allocates for each entry
/// of the central directory and bounds neither their number nor the time it
/// takes, so a hostile directory costs far more than the file weighs. 4 MiB
/// holds tens of thousands of entries, far more than a workbook has.
const DIRECTORY_BUDGET: u64 = 4 * 1024 * 1024;

/// Bytes inflated per read.
const CHUNK: usize = 32 * 1024;

/// The fixed part of a local file header and of a central directory record
/// (APPNOTE 4.3.7 and 4.3.12): the name, the extra field and, in the central
/// directory, the comment follow.
const LOCAL_HEADER: usize = 30;
const CENTRAL_RECORD: usize = 46;

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
        let budget = Budget {
            left: Cell::new(DIRECTORY_BUDGET),
            exhausted: Cell::new(false),
        };
        let reader = Metered {
            cursor: Cursor::new(bytes),
            budget: &budget,
        };
        // An Office file starts with its archive. With this offset the crate
        // does not look for the directory in the whole file, which would spend
        // the budget on any file of a few megabytes.
        let config = Config {
            archive_offset: ArchiveOffset::Known(0),
        };
        let archive = ZipArchive::with_config(config, reader)
            .map_err(|error| open_error(&error, budget.exhausted.get()))?;
        let entries = archive.metadata();
        let mut names = BTreeMap::new();
        for index in 0..entries.len() {
            let name = entry_name(&entries, index);
            if name.is_some_and(|name| names.insert(name, index).is_some()) {
                return Err(duplicate_entry());
            }
        }
        // The crate keeps the last of several entries of one name and says
        // nothing, so the records of the directory are counted again.
        let start = archive.central_directory_start();
        if count_records(bytes, start, entries.len().saturating_add(1)) > entries.len() {
            return Err(duplicate_entry());
        }
        Ok(Self {
            bytes,
            entries,
            names,
        })
    }

    /// Whether the archive has an entry called `name`.
    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.contains_key(&normalize(name))
    }

    /// The names of the entries, normalised, in order.
    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.keys().map(String::as_str)
    }

    /// The normalised name of the entry called `name`, if there is one.
    pub(super) fn canonical(&self, name: &str) -> Option<&str> {
        let (canonical, _) = self.names.get_key_value(&normalize(name))?;
        Some(canonical)
    }

    /// The content of the entry called `name`, or `None` when there is no such
    /// entry. At most `cap` bytes are inflated.
    pub(super) fn read(&self, name: &str, cap: u64) -> Result<Option<Vec<u8>>, ContainerError> {
        self.read_canonical(&normalize(name), cap)
    }

    /// The content of the entry that `canonical`, a name as [`Archive::names`]
    /// and [`Archive::canonical`] give it, stands for. Such a name is not
    /// normalised again: decoding the escapes of a name twice could lead to
    /// another entry.
    pub(super) fn read_canonical(
        &self,
        canonical: &str,
        cap: u64,
    ) -> Result<Option<Vec<u8>>, ContainerError> {
        let Some(&index) = self.names.get(canonical) else {
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
/// slash, the percent-escapes decoded, and lowercase. OPC compares part names
/// without regard to case. That `my%20file` and `my file` name one part is the
/// reader's choice, not checked against Excel.
fn normalize(name: &str) -> String {
    let name = name.replace('\\', "/");
    unescape(name.trim_start_matches('/')).to_ascii_lowercase()
}

/// `text` with its percent-escapes decoded. An escape stays as written when it
/// is not a `%` and two hexadecimal digits, or when it stands for a slash or a
/// backslash, which no segment of a part name holds. The whole text stays as
/// written when the escapes decode to bytes that are not UTF-8.
fn unescape(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let mut decoded = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        let escape = if byte == b'%' {
            byte_of_escape(tail)
        } else {
            None
        };
        if let Some(value) = escape {
            decoded.push(value);
            rest = tail.get(2..).unwrap_or_default();
        } else {
            decoded.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_owned())
}

/// The byte that the two hexadecimal digits at the start of `digits` stand
/// for, unless it is a slash or a backslash.
fn byte_of_escape(digits: &[u8]) -> Option<u8> {
    let digit = |at: usize| char::from(*digits.get(at)?).to_digit(16);
    let value = digit(0)?.checked_mul(16)?.checked_add(digit(1)?)?;
    u8::try_from(value)
        .ok()
        .filter(|value| !matches!(value, b'/' | b'\\'))
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

/// The length of the central directory record at `at`, if there is one.
fn record_length(bytes: &[u8], at: usize) -> Option<usize> {
    let record = bytes.get(at..at.checked_add(CENTRAL_RECORD)?)?;
    if !record.starts_with(b"PK\x01\x02") {
        return None;
    }
    let variable = |length: usize, field| length.checked_add(u16_at(record, field)?);
    variable(variable(variable(CENTRAL_RECORD, 28)?, 30)?, 32)
}

/// How many records follow one another from `start`, counting up to `limit`.
fn count_records(bytes: &[u8], start: u64, limit: usize) -> usize {
    let first = usize::try_from(start).ok();
    let starts = std::iter::successors(first, |&at| at.checked_add(record_length(bytes, at)?));
    starts
        .take(limit)
        .take_while(|&at| record_length(bytes, at).is_some())
        .count()
}

fn open_error(error: &ZipError, directory_too_large: bool) -> ContainerError {
    if directory_too_large {
        let message = "the central directory of the archive is too large";
        return fail(
            Status::SuspiciousArchive,
            "zip.directory_too_large",
            message,
        );
    }
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

/// What is left to read while an archive opens, and whether it ran out.
struct Budget {
    left: Cell<u64>,
    exhausted: Cell<bool>,
}

/// An in-memory reader that fails once its budget is spent.
struct Metered<'a> {
    cursor: Cursor<&'a [u8]>,
    budget: &'a Budget,
}

impl Read for Metered<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.cursor.read(buffer)?;
        let read_bytes = u64::try_from(read).unwrap_or(u64::MAX);
        let Some(left) = self.budget.left.get().checked_sub(read_bytes) else {
            self.budget.exhausted.set(true);
            return Err(io::Error::other("the directory budget is spent"));
        };
        self.budget.left.set(left);
        Ok(read)
    }
}

impl Seek for Metered<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.cursor.seek(position)
    }
}
