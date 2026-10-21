// SPDX-License-Identifier: Apache-2.0
//! A workbook with a part taken out or changed: the report keeps what can be
//! read, and says why the rest cannot be, or that there was nothing to read.
//! The files are the `.xls` fixture of one workbook, with a stream or a
//! storage of its compound file taken out or put back changed.

// `clippy.toml` bans the core from writing compound files. These tests change
// the one of a fixture.
#![allow(clippy::disallowed_methods)]

use std::io::{Cursor, Write};

use cfb::CompoundFile;
use macroport_core::report::{Status, Xlm};
use macroport_testkit::corpus;

use crate::support::{codes, extraction};

type Compound = CompoundFile<Cursor<Vec<u8>>>;

/// The `.xls` fixture of a workbook with three modules, after `edit`.
fn workbook(edit: impl FnOnce(&mut Compound)) -> Vec<u8> {
    let fixture = corpus::read("fixtures/generated/01-01-hide-unhide-columns-find.xls");
    let mut compound = CompoundFile::open(Cursor::new(fixture)).expect("a compound file");
    edit(&mut compound);
    compound.flush().expect("a flush to memory");
    compound.into_inner().into_inner()
}

/// Puts `data` in the stream at `path`, in place of what it holds.
fn replace_stream(compound: &mut Compound, path: &str, data: &[u8]) {
    compound.remove_stream(path).expect("the stream is there");
    let mut stream = compound.create_stream(path).expect("a stream");
    stream.write_all(data).expect("a write to memory");
}

#[test]
fn a_workbook_stream_cut_short_is_corrupted_and_the_project_is_still_read() {
    // A BOF record that claims 16 bytes of data, and holds one.
    let cut = workbook(|c| replace_stream(c, "/Workbook", &[0x09, 0x08, 0x10, 0x00, 0x00]));

    let extraction = extraction(&cut, "a.xls");

    let report = &extraction.file;
    assert_eq!(report.status, Status::Corrupted);
    assert_eq!(codes(report), ["biff.truncated"]);
    assert_eq!((&report.xlm, report.workbook_encrypted), (&None, None));
    assert_eq!(extraction.sources.iter().flatten().count(), 3);
}

#[test]
fn a_workbook_without_its_stream_is_corrupted_and_the_project_is_still_read() {
    let headless = workbook(|c| c.remove_stream("/Workbook").expect("the stream is there"));

    let extraction = extraction(&headless, "a.xls");

    let report = &extraction.file;
    assert_eq!(report.status, Status::Corrupted);
    assert_eq!(codes(report), ["biff.workbook.missing"]);
    assert_eq!((&report.xlm, report.workbook_encrypted), (&None, None));
    assert_eq!(extraction.sources.iter().flatten().count(), 3);
}

#[test]
fn a_compound_file_that_is_not_a_workbook_is_unsupported() {
    let other = workbook(|c| {
        c.remove_stream("/Workbook").expect("the stream is there");
        c.remove_storage_all("/_VBA_PROJECT_CUR")
            .expect("the storage is there");
    });

    let report = extraction(&other, "a.xls").file;

    assert_eq!(report.status, Status::Unsupported);
    assert!(report.vba_project.is_none());
    assert_eq!(codes(&report), ["file.unsupported_format"]);
}

#[test]
fn a_workbook_without_a_vba_storage_has_no_macros() {
    let plain = workbook(|c| {
        c.remove_storage_all("/_VBA_PROJECT_CUR")
            .expect("the storage is there");
    });

    let report = extraction(&plain, "a.xls").file;

    assert_eq!(report.status, Status::NoMacros);
    assert!(report.vba_project.is_none());
    assert_eq!(codes(&report), Vec::<&str>::new());
    let no_sheets = Xlm {
        present: false,
        macro_sheets: Some(0),
    };
    assert_eq!(report.xlm, Some(no_sheets));
    assert_eq!(report.workbook_encrypted, Some(false));
}
