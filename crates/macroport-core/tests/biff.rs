// SPDX-License-Identifier: Apache-2.0
//! The workbook globals of BIFF8 streams: the macro sheets they bind, the
//! FILEPASS record, and the streams that end too soon or hold broken records.
//! The streams are those of the `.xls` fixtures and records built by hand.

// The helpers build and cut bytes by hand. The lints against unwrapping,
// indexing and unchecked arithmetic protect the code that reads hostile
// input, and `clippy.toml` only exempts the bodies of `#[test]` functions.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use macroport_core::container::{BiffInfo, CfbFile, ContainerError, read_biff_workbook};
use macroport_core::report::Status;
use macroport_testkit::corpus;

const BOF: u16 = 0x0809;
const EOF: u16 = 0x000A;
const FILEPASS: u16 = 0x002F;
const BOUND_SHEET: u16 = 0x0085;

const VISIBLE: u8 = 0;
const HIDDEN: u8 = 1;
const VERY_HIDDEN: u8 = 2;

const WORKSHEET: u8 = 0;
const MACRO_SHEET: u8 = 1;
const CHART_SHEET: u8 = 2;
const VBA_MODULE: u8 = 6;

const MACRO_SHEET_FIXTURE: &str = "fixtures/generated/xlm-01-macrosheet.xls";
const ENCRYPTED_FIXTURE: &str = "fixtures/generated/enc-02-open-password.xls";

/// A record: its type, its size, its data.
fn record(kind: u16, data: &[u8]) -> Vec<u8> {
    let size = u16::try_from(data.len()).unwrap();
    [&kind.to_le_bytes()[..], &size.to_le_bytes(), data].concat()
}

/// The BOF record that opens the globals of a BIFF8 workbook.
fn bof() -> Vec<u8> {
    let data = [
        0x00, 0x06, 0x05, 0x00, 0x5A, 0x4F, 0xCD, 0x07, 0xC1, 0x00, 0x02, 0x00, 0x06, 0x08, 0x00,
        0x00,
    ];
    record(BOF, &data)
}

/// A BoundSheet8 record: the position of the sheet, its state, its type and
/// its name, `S`.
fn bound_sheet_in_state(state: u8, sheet_type: u8) -> Vec<u8> {
    record(BOUND_SHEET, &[0, 0, 0, 0, state, sheet_type, 1, 0, b'S'])
}

/// A visible sheet.
fn bound_sheet(sheet_type: u8) -> Vec<u8> {
    bound_sheet_in_state(VISIBLE, sheet_type)
}

fn read(stream: &[u8]) -> BiffInfo {
    read_biff_workbook(stream).unwrap_or_else(|error| panic!("the stream should read: {error}"))
}

fn refused(stream: &[u8]) -> ContainerError {
    read_biff_workbook(stream).expect_err("the stream should be refused")
}

fn assert_error(error: &ContainerError, code: &str) {
    assert_eq!(
        (error.status, error.code),
        (Status::Corrupted, code),
        "{error}"
    );
}

/// The `Workbook` stream of a fixture, by its path under `corpus/`.
fn workbook_stream(path: &str) -> Vec<u8> {
    let bytes = corpus::read(path);
    let mut file = CfbFile::open(&bytes).unwrap();
    file.read_stream("Workbook")
        .unwrap()
        .unwrap_or_else(|| panic!("{path} has no Workbook stream"))
}

/// Where the first record of `record_type` ends in a stream of whole records:
/// the record headers are never encrypted, so this reads FILEPASS workbooks
/// as well.
fn end_of_first(stream: &[u8], record_type: u16) -> usize {
    let mut at = 0;
    loop {
        let found = u16::from_le_bytes([stream[at], stream[at + 1]]);
        let size = u16::from_le_bytes([stream[at + 2], stream[at + 3]]);
        at += 4 + usize::from(size);
        if found == record_type {
            return at;
        }
    }
}

#[test]
fn the_macro_sheets_are_the_sheets_of_type_one() {
    let stream = [
        bof(),
        bound_sheet(WORKSHEET),
        bound_sheet(MACRO_SHEET),
        bound_sheet(CHART_SHEET),
        bound_sheet(MACRO_SHEET),
        bound_sheet(VBA_MODULE),
        record(EOF, &[]),
    ]
    .concat();

    let info = read(&stream);
    assert_eq!(info.macro_sheets, Some(2));
    assert!(!info.file_pass);
}

#[test]
fn hidden_macro_sheets_count_like_the_others() {
    // Only the type tells a macro sheet; the state says who may see it.
    let stream = [
        bof(),
        bound_sheet_in_state(VISIBLE, MACRO_SHEET),
        bound_sheet_in_state(HIDDEN, MACRO_SHEET),
        bound_sheet_in_state(VERY_HIDDEN, MACRO_SHEET),
        bound_sheet_in_state(VERY_HIDDEN, WORKSHEET),
        record(EOF, &[]),
    ]
    .concat();

    assert_eq!(read(&stream).macro_sheets, Some(3));
}

#[test]
fn a_workbook_without_sheets_binds_no_macro_sheet() {
    let info = read(&[bof(), record(EOF, &[])].concat());

    assert_eq!(info.macro_sheets, Some(0));
}

#[test]
fn a_filepass_record_hides_the_sheets() {
    // After FILEPASS the data of the records is encrypted: whatever the
    // sheets say, they cannot be told apart.
    let stream = [
        bof(),
        record(FILEPASS, &[0x01, 0x00, 0x04, 0x00, 0x02, 0x00]),
        bound_sheet(MACRO_SHEET),
        record(EOF, &[]),
    ]
    .concat();

    let info = read(&stream);
    assert!(info.file_pass);
    assert_eq!(info.macro_sheets, None);
}

#[test]
fn the_sheets_after_the_globals_are_not_counted() {
    let stream = [
        bof(),
        bound_sheet(WORKSHEET),
        record(EOF, &[]),
        bof(),
        bound_sheet(MACRO_SHEET),
        record(EOF, &[]),
    ]
    .concat();

    assert_eq!(read(&stream).macro_sheets, Some(0));
}

#[test]
fn a_stream_that_does_not_start_with_a_bof_record_is_refused() {
    let no_bof = [bound_sheet(MACRO_SHEET), record(EOF, &[])].concat();
    for stream in [&[][..], &[0x09], &no_bof, b"not a workbook"] {
        assert_error(&refused(stream), "biff.bof_missing");
    }
}

#[test]
fn a_bound_sheet_too_short_to_hold_its_type_is_refused() {
    let with_data = |data: &[u8]| [bof(), record(BOUND_SHEET, data), record(EOF, &[])].concat();

    assert_error(&refused(&with_data(&[])), "biff.boundsheet");
    assert_error(&refused(&with_data(&[0, 0, 0, 0, 0])), "biff.boundsheet");
    // The type is the sixth byte, whatever follows.
    let info = read(&with_data(&[0, 0, 0, 0, 0, MACRO_SHEET]));
    assert_eq!(info.macro_sheets, Some(1));
}

#[test]
fn records_cut_short_are_refused() {
    let whole = [bof(), bound_sheet(WORKSHEET), record(EOF, &[])].concat();
    let bof_len = bof().len();
    // A record that claims 65,535 bytes and holds two.
    let too_big = [bof(), vec![0x85, 0x00, 0xFF, 0xFF, 0x01, 0x02]].concat();

    for stream in [
        &bof()[..2],
        &bof()[..10],
        &whole[..bof_len + 3],
        &whole[..bof_len + 8],
        &too_big,
        // Whole records, but no EOF record to end the globals.
        &bof(),
        &whole[..whole.len() - 4],
    ] {
        assert_error(&refused(stream), "biff.truncated");
    }
}

#[test]
fn the_xls_fixtures_give_their_macro_sheets() {
    let names = corpus::files("fixtures/generated", &["xls"]);
    assert!(names.len() > 70, "{} workbooks", names.len());
    for name in names {
        let expected = match name.as_str() {
            MACRO_SHEET_FIXTURE => (false, Some(1)),
            ENCRYPTED_FIXTURE => (true, None),
            _ => (false, Some(0)),
        };
        let info = read(&workbook_stream(&name));
        assert_eq!((info.file_pass, info.macro_sheets), expected, "{name}");
    }
}

/// Every prefix of `stream` shorter than `needed` is refused, and the others
/// read as `expected`.
fn assert_prefixes(stream: &[u8], needed: usize, expected: BiffInfo) {
    for length in 0..stream.len() {
        let prefix = &stream[..length];
        if length >= needed {
            assert_eq!(read(prefix), expected, "{length} bytes");
        } else if length < 2 {
            assert_error(&refused(prefix), "biff.bof_missing");
        } else {
            assert_error(&refused(prefix), "biff.truncated");
        }
    }
}

#[test]
fn a_real_workbook_cut_anywhere_in_its_globals_is_refused() {
    let stream = workbook_stream(MACRO_SHEET_FIXTURE);
    let expected = BiffInfo {
        file_pass: false,
        macro_sheets: Some(1),
    };

    assert_prefixes(&stream, end_of_first(&stream, EOF), expected);
}

#[test]
fn an_encrypted_workbook_reads_once_its_filepass_record_is_whole() {
    let stream = workbook_stream(ENCRYPTED_FIXTURE);
    let expected = BiffInfo {
        file_pass: true,
        macro_sheets: None,
    };

    assert_prefixes(&stream, end_of_first(&stream, FILEPASS), expected);
}

#[test]
fn a_byte_changed_anywhere_in_the_globals_never_panics_the_reader() {
    for path in [MACRO_SHEET_FIXTURE, ENCRYPTED_FIXTURE] {
        let stream = workbook_stream(path);
        for at in 0..end_of_first(&stream, EOF) {
            for value in [0x00, 0x7F, 0xFF] {
                let mut bytes = stream.clone();
                bytes[at] = value;
                if let Err(error) = read_biff_workbook(&bytes) {
                    assert!(error.code.starts_with("biff."), "{path} {at}: {error}");
                    assert_eq!(error.status, Status::Corrupted, "{path} {at}: {error}");
                }
            }
        }
    }
}
