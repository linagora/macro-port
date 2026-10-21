// SPDX-License-Identifier: Apache-2.0
//! The `Workbook` stream of an `.xls` file (BIFF8, MS-XLS): the two things
//! that the container layer reads from it.
//!
//! The stream is a sequence of records: a 16-bit type, a 16-bit size and that
//! many bytes of data, little-endian. Its first substream, the workbook
//! globals, runs from a BOF record to the next EOF record. It holds one
//! BoundSheet8 record per sheet, whose `dt` byte gives the type of the sheet:
//! 1 is a macro sheet (Excel 4.0 macros, XLM). A FILEPASS record means that the
//! data of the records after it is encrypted, the `dt` bytes among it, so the
//! macro sheets cannot be counted. The VBA project is stored apart from this
//! stream and is not encrypted.
//!
//! | code | status | meaning |
//! | --- | --- | --- |
//! | `biff.bof_missing` | `corrupted` | the stream does not start with a BOF record |
//! | `biff.truncated` | `corrupted` | a record is cut short, or the globals have no EOF record |
//! | `biff.boundsheet` | `corrupted` | a BoundSheet8 record is too short to hold its type |

use super::ContainerError;
use crate::report::Status;

const BOF: u16 = 0x0809;
const EOF: u16 = 0x000A;
const FILEPASS: u16 = 0x002F;
const BOUND_SHEET: u16 = 0x0085;

/// Where `dt`, the type of the sheet, sits in the data of a BoundSheet8
/// record: after `lbPlyPos` (4 bytes) and `hsState` (1 byte).
const SHEET_TYPE_AT: usize = 5;
const MACRO_SHEET: u8 = 1;

/// The type and the size of a record, ahead of its data.
const RECORD_HEADER_LEN: usize = 4;

/// What the container layer reads from a `Workbook` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiffInfo {
    /// A FILEPASS record encrypts the workbook. The VBA project is not
    /// affected.
    pub file_pass: bool,
    /// How many sheets are macro sheets (XLM), or `None` when the workbook is
    /// encrypted and the types of its sheets cannot be read.
    pub macro_sheets: Option<u32>,
}

/// Reads the workbook globals of a `Workbook` stream: whether it is encrypted
/// and how many macro sheets it binds.
///
/// Only the globals are read, up to their EOF record. A FILEPASS record ends
/// the scan at once: what follows it is encrypted.
///
/// # Errors
///
/// A [`ContainerError`] with a `biff.*` code (see the module documentation)
/// when the stream does not start with a BOF record, is not made of whole
/// records, ends before the EOF record of the globals, or holds a BoundSheet8
/// record too short to give the type of its sheet.
pub fn read_biff_workbook(stream: &[u8]) -> Result<BiffInfo, ContainerError> {
    // The type alone tells a workbook from anything else: a BOF record cut
    // short is a different failure.
    if !stream.starts_with(&BOF.to_le_bytes()) {
        return Err(corrupted(
            "biff.bof_missing",
            "the stream does not start with a BOF record",
        ));
    }
    let mut macro_sheets = 0_u32;
    let records = Records {
        rest: stream,
        offset: 0,
    };
    for record in records {
        let record = record?;
        match record.kind {
            FILEPASS => {
                return Ok(BiffInfo {
                    file_pass: true,
                    macro_sheets: None,
                });
            }
            EOF => {
                return Ok(BiffInfo {
                    file_pass: false,
                    macro_sheets: Some(macro_sheets),
                });
            }
            BOUND_SHEET => match record.data.get(SHEET_TYPE_AT) {
                Some(&MACRO_SHEET) => macro_sheets = macro_sheets.saturating_add(1),
                Some(_) => {}
                None => {
                    return Err(corrupted(
                        "biff.boundsheet",
                        format!(
                            "the BoundSheet8 record at offset {} holds {} bytes: too few for its sheet type",
                            record.offset,
                            record.data.len()
                        ),
                    ));
                }
            },
            _ => {}
        }
    }
    Err(corrupted(
        "biff.truncated",
        "the workbook globals end without an EOF record",
    ))
}

fn corrupted(code: &'static str, message: impl Into<String>) -> ContainerError {
    ContainerError::new(Status::Corrupted, code, message)
}

/// A record of the stream.
struct Record<'a> {
    kind: u16,
    data: &'a [u8],
    /// Where the record starts in the stream, for the messages.
    offset: usize,
}

/// The records of a stream, up to the first one that is cut short.
struct Records<'a> {
    rest: &'a [u8],
    /// Where `rest` starts in the stream.
    offset: usize,
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, ContainerError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let offset = self.offset;
        let record = self.split_record();
        if record.is_err() {
            // Nothing can be read after a record cut short.
            self.rest = &[];
        }
        Some(record.map(|(kind, data)| Record { kind, data, offset }))
    }
}

impl<'a> Records<'a> {
    /// Takes the next record off `rest`: its type and its data.
    fn split_record(&mut self) -> Result<(u16, &'a [u8]), ContainerError> {
        let Some((&[kind_low, kind_high, size_low, size_high], after)) =
            self.rest.split_first_chunk::<RECORD_HEADER_LEN>()
        else {
            return Err(corrupted(
                "biff.truncated",
                format!(
                    "the record header at offset {} is cut short: {} bytes of {RECORD_HEADER_LEN}",
                    self.offset,
                    self.rest.len()
                ),
            ));
        };
        let size = usize::from(u16::from_le_bytes([size_low, size_high]));
        let Some((data, rest)) = after.split_at_checked(size) else {
            return Err(corrupted(
                "biff.truncated",
                format!(
                    "the record at offset {} claims {size} bytes of data and the stream holds {}",
                    self.offset,
                    after.len()
                ),
            ));
        };
        self.rest = rest;
        self.offset = self
            .offset
            .saturating_add(RECORD_HEADER_LEN)
            .saturating_add(size);
        Ok((u16::from_le_bytes([kind_low, kind_high]), data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_records_end_after_one_that_is_cut_short() {
        // A BoundSheet8 record that claims 65,535 bytes and holds one.
        let mut records = Records {
            rest: &[0x85, 0x00, 0xFF, 0xFF, 0x01],
            offset: 0,
        };

        assert!(matches!(records.next(), Some(Err(_))));
        assert!(records.next().is_none());
    }
}
