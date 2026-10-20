// SPDX-License-Identifier: Apache-2.0
//! The `dir` stream of a VBA project ([MS-OVBA] 2.3.4.2): the properties of
//! the project, its references and its modules.
//!
//! The stream is a run of records, each an id (2 bytes), a size (4 bytes) and
//! that many bytes of data, so a record that is not needed is skipped by its
//! size. Three things depart from that pattern:
//!
//! - PROJECTVERSION declares a size of 4 and is followed by 6 bytes.
//! - Some records of a module have no data: where the others have a size,
//!   they have a reserved field, which Office ignores.
//! - A "reserved" marker splits some records in two, a name then its Unicode
//!   form. The marker is the id of the second half, which is then a record in
//!   its own right.

use super::reader::{Reader, le_u16, le_u32};
use super::text::{self, RawName};
use super::{MAX_DIR_STREAM, MAX_MODULES, OvbaError, decompress};
use crate::report::Status;

const PROJECT_CODE_PAGE: u16 = 0x0003;
const PROJECT_NAME: u16 = 0x0004;
const PROJECT_VERSION: u16 = 0x0009;
const TERMINATOR: u16 = 0x0010;
const MODULE_NAME: u16 = 0x0019;
const MODULE_NAME_UNICODE: u16 = 0x0047;
const MODULE_STREAM_NAME: u16 = 0x001A;
/// The marker that opens the Unicode half of a module stream name.
const MODULE_STREAM_NAME_UNICODE: u16 = 0x0032;
const MODULE_OFFSET: u16 = 0x0031;
const MODULE_PROCEDURAL: u16 = 0x0021;
const MODULE_NON_PROCEDURAL: u16 = 0x0022;
const MODULE_READ_ONLY: u16 = 0x0025;
const MODULE_PRIVATE: u16 = 0x0028;
const MODULE_TERMINATOR: u16 = 0x002B;

/// What follows the id of a PROJECTVERSION record: a reserved field (4
/// bytes), the major version (4) and the minor version (2).
const VERSION_FIELDS: usize = 10;
/// The reserved field that follows the id of a record that has no data.
const RESERVED: usize = 4;

/// What the `dir` stream says about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirInfo {
    /// The code page of the MBCS text of the project, if the stream gives one.
    pub code_page: Option<u16>,
    /// The name of the project.
    pub project_name: Option<String>,
    /// The modules, in the order of the stream.
    pub modules: Vec<DirModule>,
}

/// A module, as the `dir` stream declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirModule {
    /// The name that code uses for the module.
    pub name: String,
    /// The name of the stream of the `VBA` storage that holds the module.
    pub stream_name: String,
    /// Where the compressed source starts in that stream. The bytes before it
    /// are the performance cache.
    pub text_offset: u32,
    /// Whether the module is procedural, that is a standard module. Document,
    /// class and designer modules share the other type, which the `PROJECT`
    /// stream tells apart.
    pub procedural: bool,
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
/// that the project has once wins. The Unicode name of a module and of its
/// stream wins over the MBCS one, which is decoded in the code page of the
/// project.
///
/// # Errors
///
/// An error if the stream is over [`MAX_DIR_STREAM`], if a record declares
/// more bytes than the stream holds, if the terminator is missing, or if the
/// project has more than [`MAX_MODULES`] modules.
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
            id @ (MODULE_PROCEDURAL
            | MODULE_NON_PROCEDURAL
            | MODULE_READ_ONLY
            | MODULE_PRIVATE
            | MODULE_TERMINATOR) => {
                reader.skip(RESERVED)?;
                parser.record(id, &[])?;
            }
            id => {
                let data = reader.record_data(id)?;
                parser.record(id, data)?;
            }
        }
    }
    parser.finish()
}

/// The records of a module, before the code page is known to apply to its
/// names.
#[derive(Debug, Default)]
struct RawModule {
    name: RawName,
    stream_name: RawName,
    text_offset: u32,
    procedural: bool,
}

impl RawModule {
    /// Takes in a record of the module, past its id and size.
    fn record(&mut self, id: u16, data: &[u8]) {
        match id {
            MODULE_NAME_UNICODE => self.name.unicode = data.to_vec(),
            MODULE_STREAM_NAME => self.stream_name.mbcs = data.to_vec(),
            MODULE_STREAM_NAME_UNICODE => self.stream_name.unicode = data.to_vec(),
            MODULE_OFFSET => {
                if let Some(offset) = le_u32(data) {
                    self.text_offset = offset;
                }
            }
            MODULE_PROCEDURAL | MODULE_NON_PROCEDURAL => {
                self.procedural = id == MODULE_PROCEDURAL;
            }
            _ => {}
        }
    }

    /// The module, or `None` if the stream names neither it nor its stream.
    fn finish(&self, code_page: Option<u16>) -> Option<DirModule> {
        let (name, stream_name) =
            match (self.name.text(code_page), self.stream_name.text(code_page)) {
                (Some(name), Some(stream_name)) => (name, stream_name),
                (Some(name), None) => (name.clone(), name),
                (None, Some(stream_name)) => (stream_name.clone(), stream_name),
                (None, None) => return None,
            };
        Some(DirModule {
            name,
            stream_name,
            text_offset: self.text_offset,
            procedural: self.procedural,
        })
    }
}

/// The records read so far.
#[derive(Debug, Default)]
struct Parser {
    code_page: Option<u16>,
    project_name: Option<Vec<u8>>,
    /// The module whose records are being read.
    module: Option<RawModule>,
    modules: Vec<RawModule>,
}

impl Parser {
    /// Takes in a record, past its id and size.
    fn record(&mut self, id: u16, data: &[u8]) -> Result<(), OvbaError> {
        match id {
            PROJECT_CODE_PAGE => self.code_page = self.code_page.or(le_u16(data)),
            PROJECT_NAME => {
                self.project_name.get_or_insert_with(|| data.to_vec());
            }
            MODULE_NAME => {
                self.finish_module()?;
                self.module = Some(RawModule {
                    name: RawName {
                        mbcs: data.to_vec(),
                        unicode: Vec::new(),
                    },
                    ..RawModule::default()
                });
            }
            MODULE_TERMINATOR => self.finish_module()?,
            // Any other record of a module belongs to the module being read.
            _ => {
                if let Some(module) = &mut self.module {
                    module.record(id, data);
                }
            }
        }
        Ok(())
    }

    fn finish_module(&mut self) -> Result<(), OvbaError> {
        let Some(module) = self.module.take() else {
            return Ok(());
        };
        if self.modules.len() >= MAX_MODULES {
            return Err(OvbaError::new(
                Status::TooLarge,
                "ovba.dir.too_many_modules",
                format!("the project declares more than {MAX_MODULES} modules"),
            ));
        }
        self.modules.push(module);
        Ok(())
    }

    /// The project, once the terminator is read: the MBCS text is decoded
    /// now that the last word on the code page is known.
    fn finish(mut self) -> Result<DirInfo, OvbaError> {
        self.finish_module()?;
        let code_page = self.code_page;
        Ok(DirInfo {
            code_page,
            project_name: self
                .project_name
                .map(|name| text::decode_mbcs(&name, code_page))
                .filter(|name| !name.is_empty()),
            modules: self
                .modules
                .iter()
                .filter_map(|module| module.finish(code_page))
                .collect(),
        })
    }
}
