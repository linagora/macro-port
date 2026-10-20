// SPDX-License-Identifier: Apache-2.0
//! The `dir` stream of a VBA project ([MS-OVBA] 2.3.4.2): the properties of
//! the project, its references and its modules.
//!
//! The stream is a run of records, each an id (2 bytes), a size (4 bytes) and
//! that many bytes of data, so a record that is not needed is skipped by its
//! size. PROJECTVERSION departs from that pattern: it declares a size of 4 and
//! is followed by 6 bytes.

use super::reader::{Reader, le_u16};
use super::text;
use super::{MAX_DIR_STREAM, OvbaError, decompress};
use crate::report::Status;

const PROJECT_CODE_PAGE: u16 = 0x0003;
const PROJECT_NAME: u16 = 0x0004;
const PROJECT_VERSION: u16 = 0x0009;
const TERMINATOR: u16 = 0x0010;

/// What follows the id of a PROJECTVERSION record: a reserved field (4
/// bytes), the major version (4) and the minor version (2).
const VERSION_FIELDS: usize = 10;

/// What the `dir` stream says about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirInfo {
    /// The code page of the MBCS text of the project, if the stream gives one.
    pub code_page: Option<u16>,
    /// The name of the project.
    pub project_name: Option<String>,
}

/// Decompresses a `dir` stream and reads it.
///
/// # Errors
///
/// An error if the stream does not decompress within [`MAX_DIR_STREAM`], or
/// [`parse_dir`] fails.
pub fn parse_compressed_dir(compressed: &[u8]) -> Result<DirInfo, OvbaError> {
    parse_dir(&decompress(compressed, MAX_DIR_STREAM)?)
}

/// Reads a decompressed `dir` stream.
///
/// Records are taken in whatever order they come, and the first of a kind
/// that the project has once wins. The MBCS text is decoded in the code page
/// of the project.
///
/// # Errors
///
/// An error if the stream is over [`MAX_DIR_STREAM`], if a record declares
/// more bytes than the stream holds, or if the terminator is missing.
pub fn parse_dir(stream: &[u8]) -> Result<DirInfo, OvbaError> {
    if stream.len() > MAX_DIR_STREAM {
        return Err(OvbaError::new(
            Status::SuspiciousArchive,
            "ovba.dir.too_large",
            format!("the dir stream is over {MAX_DIR_STREAM} bytes"),
        ));
    }
    let mut reader = Reader::new(stream);
    let mut parser = Parser::default();
    loop {
        match reader.u16()? {
            TERMINATOR => break,
            PROJECT_VERSION => reader.skip(VERSION_FIELDS)?,
            id => {
                let data = reader.record_data(id)?;
                parser.record(id, data);
            }
        }
    }
    Ok(parser.finish())
}

/// The records read so far.
#[derive(Debug, Default)]
struct Parser {
    code_page: Option<u16>,
    project_name: Option<Vec<u8>>,
}

impl Parser {
    /// Takes in a record, past its id and size.
    fn record(&mut self, id: u16, data: &[u8]) {
        match id {
            PROJECT_CODE_PAGE => self.code_page = self.code_page.or(le_u16(data)),
            PROJECT_NAME => {
                self.project_name.get_or_insert_with(|| data.to_vec());
            }
            _ => {}
        }
    }

    /// The project, once the terminator is read: the MBCS text is decoded
    /// now that the last word on the code page is known.
    fn finish(self) -> DirInfo {
        let code_page = self.code_page;
        DirInfo {
            code_page,
            project_name: self
                .project_name
                .map(|name| text::decode_mbcs(&name, code_page))
                .filter(|name| !name.is_empty()),
        }
    }
}
