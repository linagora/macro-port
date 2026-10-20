// SPDX-License-Identifier: Apache-2.0
//! Compound File Binary containers (MS-CFB): `.xls` workbooks, the encrypted
//! OOXML packages and the `vbaProject.bin` of the OOXML formats.
//!
//! The `cfb` crate (ADR-0002) parses the container. This module opens only a
//! byte slice, holds the limits of SPEC M1 (2,000 directory entries, storage
//! depth 32, streams of at most 64 MB, chain loops are errors) and turns every
//! failure into a [`ContainerError`] with a stable code:
//!
//! | code | status | meaning |
//! | --- | --- | --- |
//! | `cfb.signature` | `corrupted` | the bytes do not start with the CFB signature |
//! | `cfb.header` | `corrupted` | the header is cut short or holds unsupported values |
//! | `cfb.fat` | `corrupted` | the (Mini)FAT or DIFAT is inconsistent, or points beyond the file, as a cut file's does |
//! | `cfb.fat_loop` | `corrupted` | a chain of sectors loops, or a sector belongs to two chains |
//! | `cfb.directory` | `corrupted` | the directory cannot be read or its tree is not a tree |
//! | `cfb.truncated` | `corrupted` | a sector or a stream is cut short: the data ends before its structure says |
//! | `cfb.malformed` | `corrupted` | any other failure of the `cfb` crate |
//! | `cfb.too_many_entries` | `corrupted` | more than [`MAX_DIRECTORY_ENTRIES`] entries in the directory tree |
//! | `cfb.too_deep` | `corrupted` | storages nested deeper than [`MAX_STORAGE_DEPTH`] |
//! | `cfb.stream_too_large` | `too_large` | a stream above [`MAX_STREAM_SIZE`] |
//!
//! A stream above its size cap is the only limit that gives `too_large`,
//! which SPEC M1 ties to sizes. The other limits describe a structure no
//! Office program writes, so a file that crosses one is `corrupted`.

use std::fmt;
use std::io::{self, Cursor, Read};
use std::path::PathBuf;

use ::cfb::{CompoundFile, Entry};

use super::ContainerError;
use crate::report::Status;

/// SPEC M1: the most entries a container's directory tree may hold, the root
/// included.
pub const MAX_DIRECTORY_ENTRIES: usize = 2_000;

/// SPEC M1: the deepest storage nesting a container may have. A storage in
/// the root is at depth 1.
pub const MAX_STORAGE_DEPTH: usize = 32;

/// SPEC M1: the largest stream, in bytes, that [`CfbFile::read_stream`] reads.
pub const MAX_STREAM_SIZE: u64 = 64 * 1024 * 1024;

const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const HEADER_LEN: usize = 512;

/// A Compound File Binary container read from a byte slice.
///
/// It reads streams by path and nothing else: the core has no use for the
/// file's timestamps or CLSIDs, and never writes.
pub struct CfbFile<'a> {
    file: CompoundFile<Cursor<&'a [u8]>>,
}

// Not derived: the `Debug` of a `Cursor` over a slice prints every byte.
impl fmt::Debug for CfbFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CfbFile").finish_non_exhaustive()
    }
}

impl<'a> CfbFile<'a> {
    /// Opens the container held by `bytes`.
    ///
    /// # Errors
    ///
    /// A [`ContainerError`] when the bytes are not a container, when its
    /// structure is broken, or when it crosses a limit of SPEC M1 (see the
    /// module documentation for the codes).
    pub fn open(bytes: &'a [u8]) -> Result<Self, ContainerError> {
        check_header(bytes)?;
        let file = CompoundFile::open(Cursor::new(bytes)).map_err(container_error)?;
        check_tree_limits(&file)?;
        Ok(Self { file })
    }

    /// Reads the stream at `path`: the names from the root to the stream, in
    /// order, separated by `/`, such as `_VBA_PROJECT_CUR/VBA/dir`. Names
    /// compare without regard to case (the rule of MS-CFB, which the `cfb`
    /// crate applies approximately beyond ASCII). A leading `/` is ignored.
    ///
    /// Returns `Ok(None)` when no stream has that path: the path names a
    /// storage or nothing, or it is not a path a container can hold (an empty
    /// name, `.` or `..`, or a name with `\`, `:` or `!`).
    ///
    /// # Errors
    ///
    /// A [`ContainerError`] when the stream is above [`MAX_STREAM_SIZE`]
    /// (`cfb.stream_too_large`, checked before any byte is read) or when its
    /// sectors cannot be read: `cfb.truncated`, `cfb.fat_loop`, `cfb.fat` or,
    /// for a failure that the module documentation does not list,
    /// `cfb.malformed`.
    pub fn read_stream(&mut self, path: &str) -> Result<Option<Vec<u8>>, ContainerError> {
        let Some(lookup) = lookup_path(path) else {
            return Ok(None);
        };
        let entry = match self.file.entry(&lookup) {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(container_error(error)),
        };
        if !entry.is_stream() {
            return Ok(None);
        }
        let len = entry.len();
        if len > MAX_STREAM_SIZE {
            return Err(ContainerError::new(
                Status::TooLarge,
                "cfb.stream_too_large",
                format!("a stream of {len} bytes is above the limit of {MAX_STREAM_SIZE} bytes"),
            ));
        }
        // An empty stream may carry any start sector: do not follow it.
        if len == 0 {
            return Ok(Some(Vec::new()));
        }
        let mut stream = self.file.open_stream(&lookup).map_err(container_error)?;
        let mut data = Vec::new();
        stream.read_to_end(&mut data).map_err(container_error)?;
        Ok(Some(data))
    }

    /// Whether a stream sits at `path`, which reads as for
    /// [`read_stream`](Self::read_stream).
    pub fn has_stream(&self, path: &str) -> bool {
        lookup_path(path).is_some_and(|lookup| self.file.is_stream(lookup))
    }

    /// Whether a storage sits at `path`, which reads as for
    /// [`read_stream`](Self::read_stream). The empty path is the root storage.
    pub fn has_storage(&self, path: &str) -> bool {
        lookup_path(path).is_some_and(|lookup| self.file.is_storage(lookup))
    }
}

/// Checks that `bytes` start with the signature and hold a whole header. The
/// crate reports both with a message that says little more.
fn check_header(bytes: &[u8]) -> Result<(), ContainerError> {
    if bytes.get(..SIGNATURE.len()) != Some(&SIGNATURE[..]) {
        return Err(ContainerError::new(
            Status::Corrupted,
            "cfb.signature",
            "the bytes do not start with the signature of a compound file",
        ));
    }
    if bytes.len() < HEADER_LEN {
        return Err(ContainerError::new(
            Status::Corrupted,
            "cfb.header",
            format!(
                "{} bytes are too few for the {HEADER_LEN}-byte header of a compound file",
                bytes.len()
            ),
        ));
    }
    Ok(())
}

/// The path under which the `cfb` crate looks an entry up, or `None` when no
/// entry can have that path. The crate resolves `.` and `..` itself, and reads
/// `\` and `C:` as the separators and prefixes of Windows paths on that
/// platform only. It refuses to open a container with `\`, `:` or `!` in a
/// name, so these paths name nothing, whatever the platform.
fn lookup_path(path: &str) -> Option<PathBuf> {
    let mut lookup = PathBuf::from("/");
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return Some(lookup);
    }
    for name in path.split('/') {
        if name.is_empty() || name == "." || name == ".." || name.contains(['\\', ':', '!']) {
            return None;
        }
        lookup.push(name);
    }
    Some(lookup)
}

/// Enforces the limits that need the directory tree: the entries it reaches
/// and the depth of its storages.
fn check_tree_limits<F>(file: &CompoundFile<F>) -> Result<(), ContainerError> {
    for (position, entry) in file.walk().enumerate() {
        if position >= MAX_DIRECTORY_ENTRIES {
            return Err(too_many_entries());
        }
        if entry.is_storage() && depth_of(&entry) > MAX_STORAGE_DEPTH {
            return Err(too_deep());
        }
    }
    Ok(())
}

/// How many levels down `entry` sits: a storage in the root is at depth 1.
///
/// This counts the separators of the path, not its components, because a
/// storage named `.` is a component that `Path` would drop. A name holds no
/// separator, so each one is a level. The root, whose path is the separator
/// alone, counts as 1 as well, which no limit notices.
fn depth_of(entry: &Entry) -> usize {
    let path = entry.path().as_os_str().as_encoded_bytes();
    path.iter()
        .filter(|byte| matches!(byte, b'/' | b'\\'))
        .count()
}

fn too_many_entries() -> ContainerError {
    ContainerError::new(
        Status::Corrupted,
        "cfb.too_many_entries",
        format!("the directory tree holds more than {MAX_DIRECTORY_ENTRIES} entries"),
    )
}

fn too_deep() -> ContainerError {
    ContainerError::new(
        Status::Corrupted,
        "cfb.too_deep",
        format!("storages are nested deeper than {MAX_STORAGE_DEPTH} levels"),
    )
}

/// The failure of the `cfb` crate as a [`ContainerError`]. The crate has no
/// error type of its own: its `io::Error`s carry a fixed English message,
/// which `code_of` reads. The wording is that of the pinned version. A
/// message that no case knows gets `cfb.malformed`, so a new wording costs a
/// code, not safety. A failure to allocate is a matter of size.
fn container_error(error: io::Error) -> ContainerError {
    let message = error.to_string();
    if error.kind() == io::ErrorKind::OutOfMemory {
        return ContainerError::new(Status::TooLarge, "cfb.stream_too_large", message);
    }
    ContainerError::new(Status::Corrupted, code_of(error.kind(), &message), message)
}

/// The code of a failure of the `cfb` crate, from its kind and its message.
fn code_of(kind: io::ErrorKind, message: &str) -> &'static str {
    let has = |needle: &str| message.contains(needle);
    if kind == io::ErrorKind::UnexpectedEof {
        "cfb.truncated"
    } else if has("duplicate sector") || has("pointed to twice") {
        "cfb.fat_loop"
    } else if has("Malformed directory")
        || has("Directory chain")
        || has("Directory entry")
        || has("Object name")
    {
        "cfb.directory"
    } else if has("Invalid CFB")
        || has("byte order")
        || has("version")
        || has("shift")
        || has("mini stream cutoff")
        || has("Initial DIFAT")
    {
        "cfb.header"
    } else if has("FAT") || has("next_id") || has("sector id") {
        "cfb.fat"
    } else {
        "cfb.malformed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The messages of `cfb` 0.15.0 that a broken container does not reach
    /// easily, one for each way to fail that `code_of` tells apart.
    #[test]
    fn the_messages_of_the_crate_get_their_codes() {
        #[rustfmt::skip]
        let cases = [
            ("Malformed FAT (sector 5 pointed to twice)", "cfb.fat_loop"),
            ("Directory chain includes duplicate sector index 2", "cfb.fat_loop"),
            ("Directory entry 4 is beyond the 1 directory sectors", "cfb.directory"),
            ("Object name cannot contain / character", "cfb.directory"),
            ("Incorrect mini stream cutoff (expected 4096, found 1)", "cfb.header"),
            ("next_id (7) is invalid", "cfb.fat"),
            ("invalid sector id", "cfb.fat"),
        ];
        for (message, code) in cases {
            assert_eq!(
                code_of(io::ErrorKind::InvalidData, message),
                code,
                "{message}"
            );
        }
        let eof = code_of(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer");
        assert_eq!(eof, "cfb.truncated");
    }

    #[test]
    fn a_message_the_crate_never_sent_is_malformed() {
        let error = container_error(io::Error::other("something new"));
        assert_eq!(
            (error.status, error.code),
            (Status::Corrupted, "cfb.malformed")
        );
    }

    #[test]
    fn running_out_of_memory_is_a_matter_of_size() {
        let error = container_error(io::Error::from(io::ErrorKind::OutOfMemory));
        assert_eq!(
            (error.status, error.code),
            (Status::TooLarge, "cfb.stream_too_large")
        );
    }
}
