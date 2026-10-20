// SPDX-License-Identifier: Apache-2.0
//! The `dir` stream: the project information of the MS-OVBA examples, the
//! quirks that it tolerates, its limits and hostile input.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

mod ovba_common;

use macroport_core::ovba::{DirInfo, MAX_DIR_STREAM, parse_compressed_dir, parse_dir};
use macroport_core::report::Status;
use ovba_common::Lcg;

// Builders for the records of the `dir` stream. The ones that take references
// and modules leave them empty until the readers of those arrive.

/// A length as 4 bytes.
fn size(data: &[u8]) -> [u8; 4] {
    u32::try_from(data.len()).unwrap().to_le_bytes()
}

/// A record: its id, its size and its data.
fn record(id: u16, data: &[u8]) -> Vec<u8> {
    [id.to_le_bytes().as_slice(), &size(data), data].concat()
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// PROJECTVERSION: its size says 4 and 6 bytes follow it.
fn version() -> Vec<u8> {
    [
        0x0009_u16.to_le_bytes().as_slice(),
        &4_u32.to_le_bytes(),
        &0x49B5_196B_u32.to_le_bytes(),
        &6_u16.to_le_bytes(),
    ]
    .concat()
}

/// The records of the project information example of the specification.
fn information_records(code_page: u16) -> Vec<Vec<u8>> {
    vec![
        record(0x0001, &1_u32.to_le_bytes()),
        record(0x0002, &0x0409_u32.to_le_bytes()),
        record(0x0014, &0x0409_u32.to_le_bytes()),
        record(0x0003, &code_page.to_le_bytes()),
        record(0x0004, b"VBAProject"),
        record(0x0005, b"Example VBA Project Description"),
        record(0x0040, &utf16("Example VBA Project Description")),
        record(0x0006, b"c:\\example path\\example.hlp"),
        record(0x003D, b"c:\\example path\\example.hlp"),
        record(0x0007, &1_u32.to_le_bytes()),
        record(0x0008, &0_u32.to_le_bytes()),
        version(),
        record(0x000C, b"abc = 1"),
        record(0x003C, &utf16("abc = 1")),
    ]
}

/// A whole `dir` stream.
fn dir(information: &[Vec<u8>], references: &[Vec<u8>], modules: &[Vec<u8>]) -> Vec<u8> {
    [
        information.concat(),
        references.concat(),
        record(0x000F, &u16::try_from(modules.len()).unwrap().to_le_bytes()),
        record(0x0013, &0xFFFF_u16.to_le_bytes()),
        modules.concat(),
        record(0x0010, b""),
    ]
    .concat()
}

fn parse(information: &[Vec<u8>], references: &[Vec<u8>], modules: &[Vec<u8>]) -> DirInfo {
    parse_dir(&dir(information, references, modules)).unwrap()
}

/// A project with its information, to which the references and the modules
/// are added in time.
fn example_stream() -> Vec<u8> {
    dir(&information_records(1252), &[], &[])
}

// The examples of the specification.

#[test]
fn reads_the_project_information_of_the_specification_example() {
    let info = parse(&information_records(0x04E4), &[], &[]);
    assert_eq!(info.code_page, Some(1252));
    assert_eq!(info.project_name.as_deref(), Some("VBAProject"));
}

// The quirks.

#[test]
fn reads_the_records_that_follow_the_version_record() {
    // The version record declares 4 bytes and holds 6: a parser that trusts
    // the size reads the minor version as the id of the next record.
    let records = [version(), record(0x0003, &1251_u16.to_le_bytes())];
    assert_eq!(parse(&records, &[], &[]).code_page, Some(1251));
}

#[test]
fn accepts_the_compat_version_record_before_the_code_page() {
    // Like any record that is not read, it is skipped by its size, wherever
    // the specification lets it come.
    let expected = parse(&information_records(1251), &[], &[]);
    for position in [1, 3] {
        let mut records = information_records(1251);
        records.insert(position, record(0x004A, &2_u32.to_le_bytes()));
        assert_eq!(parse(&records, &[], &[]), expected);
    }
}

#[test]
fn skips_an_unknown_record_by_its_size() {
    let expected = parse(&information_records(1252), &[], &[]);
    for position in [0, 5, 14] {
        let mut records = information_records(1252);
        records.insert(position, record(0x7777, b"opaque"));
        assert_eq!(parse(&records, &[], &[]), expected);
    }
}

#[test]
fn an_unknown_record_that_overruns_the_stream_is_corrupted() {
    let mut records = information_records(1252);
    records.insert(5, [0x77, 0x77, 0xFF, 0xFF, 0xFF, 0xFF, 0x01].to_vec());
    let error = parse_dir(&dir(&records, &[], &[])).unwrap_err();
    assert_eq!(error.status, Status::Corrupted);
    assert_eq!(error.code, "ovba.dir.truncated");
}

#[test]
fn a_known_record_that_overruns_the_stream_is_corrupted() {
    let mut stream = dir(&information_records(1252), &[], &[]);
    // The name of the project claims 4 GiB.
    let name = b"VBAProject";
    let at = stream
        .windows(name.len())
        .position(|window| window == name)
        .unwrap();
    for byte in stream.get_mut(at - 4..at).unwrap() {
        *byte = 0xFF;
    }
    let error = parse_dir(&stream).unwrap_err();
    assert_eq!(error.status, Status::Corrupted);
    assert_eq!(error.code, "ovba.dir.truncated");
}

#[test]
fn ignores_what_follows_the_terminator() {
    let mut stream = dir(&information_records(1252), &[], &[]);
    stream.extend([0xAA; 100]);
    assert_eq!(
        parse_dir(&stream).unwrap(),
        parse(&information_records(1252), &[], &[])
    );
}

#[test]
fn the_first_code_page_and_project_name_win() {
    let mut records = information_records(1252);
    records.push(record(0x0003, &1251_u16.to_le_bytes()));
    records.push(record(0x0004, b"Other"));
    let info = parse(&records, &[], &[]);
    assert_eq!(info.code_page, Some(1252));
    assert_eq!(info.project_name.as_deref(), Some("VBAProject"));
}

#[test]
fn a_project_without_a_code_page_has_none() {
    let mut records = information_records(1252);
    records.remove(3);
    assert_eq!(parse(&records, &[], &[]).code_page, None);
}

/// The name of the example project once its bytes are replaced. With no
/// code page, the record that gives it is left out.
fn decoded_name(code_page: Option<u16>, name: &[u8]) -> Option<String> {
    let mut records = information_records(code_page.unwrap_or(1252));
    *records.get_mut(4).unwrap() = record(0x0004, name);
    if code_page.is_none() {
        records.remove(3);
    }
    parse(&records, &[], &[]).project_name
}

#[test]
fn decodes_the_project_name_in_the_code_page() {
    let name = decoded_name(Some(1251), &[0xCF, 0xF0, 0xE8]);
    assert_eq!(name.as_deref(), Some("\u{41f}\u{440}\u{438}"));
}

#[test]
fn decodes_the_project_name_in_utf_16_for_code_page_1200() {
    let name = decoded_name(Some(1200), &utf16("VBAProject"));
    assert_eq!(name.as_deref(), Some("VBAProject"));
}

#[test]
fn decodes_the_project_name_in_windows_1252_without_a_known_code_page() {
    // 0x80 is the euro sign in windows-1252 and a control character in Latin-1.
    for code_page in [None, Some(9999)] {
        let name = decoded_name(code_page, &[0x80, 0xE9]);
        assert_eq!(name.as_deref(), Some("\u{20ac}\u{e9}"), "{code_page:?}");
    }
}

#[test]
fn does_not_take_the_start_of_a_name_for_a_byte_order_mark() {
    // FF FE would start UTF-16 and EF BB BF UTF-8, but the code page decides.
    let first = decoded_name(Some(1252), &[0xFF, 0xFE, 0x41]);
    assert_eq!(first.as_deref(), Some("\u{ff}\u{fe}A"));
    let second = decoded_name(Some(1252), &[0xEF, 0xBB, 0xBF, 0x41]);
    assert_eq!(second.as_deref(), Some("\u{ef}\u{bb}\u{bf}A"));
}

#[test]
fn an_empty_project_name_is_no_name() {
    assert_eq!(decoded_name(Some(1252), b""), None);
}

// Limits.

/// A container of raw chunks that hold `data`, the last padded with zeros.
fn stored(data: &[u8]) -> Vec<u8> {
    let mut container = vec![0x01];
    for chunk in data.chunks(4096) {
        container.extend([0xFF, 0x3F]);
        container.extend_from_slice(chunk);
        container.extend(std::iter::repeat_n(
            0,
            4096_usize.saturating_sub(chunk.len()),
        ));
    }
    container
}

#[test]
fn reads_a_compressed_dir_stream() {
    let stream = example_stream();
    assert_eq!(
        parse_compressed_dir(&stored(&stream)).unwrap(),
        parse_dir(&stream).unwrap()
    );
}

#[test]
fn accepts_a_dir_stream_of_exactly_the_limit() {
    // An unknown record pads the stream: its id and its size take 6 bytes.
    let mut records = information_records(1252);
    let unpadded = dir(&records, &[], &[]).len();
    records.push(record(0x7777, &vec![0; MAX_DIR_STREAM - unpadded - 6]));
    let stream = dir(&records, &[], &[]);
    assert_eq!(stream.len(), MAX_DIR_STREAM);
    let expected = parse(&information_records(1252), &[], &[]);
    assert_eq!(parse_dir(&stream).unwrap(), expected);
    assert_eq!(parse_compressed_dir(&stored(&stream)).unwrap(), expected);
}

#[test]
fn rejects_a_dir_stream_that_decompresses_beyond_the_limit() {
    // 4097 raw chunks of 4096 bytes are 16 MiB and a chunk.
    let container = stored(&vec![0; MAX_DIR_STREAM + 1]);
    let error = parse_compressed_dir(&container).unwrap_err();
    assert_eq!(error.status, Status::SuspiciousArchive);
    assert_eq!(error.code, "ovba.decompress.too_large");
}

#[test]
fn rejects_a_decompressed_dir_stream_over_the_limit() {
    let stream = vec![0; MAX_DIR_STREAM + 1];
    let error = parse_dir(&stream).unwrap_err();
    assert_eq!(error.status, Status::SuspiciousArchive);
    assert_eq!(error.code, "ovba.dir.too_large");
}

#[test]
fn passes_on_the_error_of_the_decompression() {
    let error = parse_compressed_dir(&[0x02, 0x00]).unwrap_err();
    assert_eq!(error.code, "ovba.decompress.bad_signature");
}

// Hostile input.

#[test]
fn an_empty_stream_is_truncated() {
    let error = parse_dir(&[]).unwrap_err();
    assert_eq!(error.status, Status::Corrupted);
    assert_eq!(error.code, "ovba.dir.truncated");
}

#[test]
fn a_truncation_error_says_where_the_stream_is_cut() {
    // The size of the record starts at offset 2 and is cut short.
    let error = parse_dir(&[0x04, 0x00, 0x0A]).unwrap_err();
    assert!(error.message.ends_with("at offset 2"), "{}", error.message);
}

#[test]
fn a_truncation_error_says_what_a_record_declares() {
    let error = parse_dir(&[0x04, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x41]).unwrap_err();
    assert!(
        error
            .message
            .contains("0x0004 declares 4294967295 bytes and 1 remain"),
        "{}",
        error.message
    );
}

#[test]
fn a_stream_cut_anywhere_before_its_terminator_is_truncated() {
    let stream = example_stream();
    // The terminator is read as soon as its id is, whatever follows.
    let complete = stream.len() - 4;
    for length in 0..complete {
        let error = parse_dir(stream.get(..length).unwrap()).unwrap_err();
        assert_eq!(error.code, "ovba.dir.truncated", "length {length}");
        assert_eq!(error.status, Status::Corrupted);
    }
    for length in complete..=stream.len() {
        parse_dir(stream.get(..length).unwrap()).unwrap();
    }
}

#[test]
fn never_panics_on_corrupted_or_random_bytes() {
    let mut lcg = Lcg::new(0x2545_F491);
    let valid = example_stream();
    for _ in 0..3_000 {
        let _ = parse_dir(&lcg.damage(&valid, 4));
        let _ = parse_dir(&lcg.random_bytes());
    }
}
