// SPDX-License-Identifier: Apache-2.0
//! A workbook with a part taken out or changed: the report keeps what can be
//! read, and says why the rest cannot be, or that there was nothing to read.
//! The files are the fixtures of one workbook, with a stream or a storage of
//! a compound file taken out or put back changed: the `.xls` itself, or the
//! VBA project of the `.xlsm`, which a new package then holds.

// `clippy.toml` bans the core from writing compound files. These tests change
// the one of a fixture.
#![allow(clippy::disallowed_methods)]

use std::io::{Cursor, Read, Write};

use cfb::CompoundFile;
use macroport_core::container::read_ooxml;
use macroport_core::ovba::{MAX_PROJECT_STREAM, parse_compressed_dir};
use macroport_core::report::{Status, Xlm};
use macroport_testkit::corpus;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::support::{codes, extraction};

type Compound = CompoundFile<Cursor<Vec<u8>>>;

/// The workbook of the fixtures: three modules, `ThisWorkbook`, `Sheet1` and
/// `M_01_01`.
const WORKBOOK: &str = "fixtures/generated/01-01-hide-unhide-columns-find";

/// The compound file `bytes`, after `edit`.
fn edited(bytes: Vec<u8>, edit: impl FnOnce(&mut Compound)) -> Vec<u8> {
    let mut compound = CompoundFile::open(Cursor::new(bytes)).expect("a compound file");
    edit(&mut compound);
    compound.flush().expect("a flush to memory");
    compound.into_inner().into_inner()
}

/// The `.xls` of the workbook, after `edit`.
fn workbook(edit: impl FnOnce(&mut Compound)) -> Vec<u8> {
    edited(corpus::read(&format!("{WORKBOOK}.xls")), edit)
}

/// A package with the VBA project of the `.xlsm` of the workbook, after `edit`,
/// and nothing else. The part is stored, not deflated: deflating a stream of
/// one byte repeated would make the package a zip bomb.
fn package(edit: impl FnOnce(&mut Compound)) -> Vec<u8> {
    let package = read_ooxml(&corpus::read(&format!("{WORKBOOK}.xlsm"))).expect("a package");
    let project = package.vba_project.expect("a VBA project");
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("xl/vbaProject.bin", options)
        .expect("a part");
    zip.write_all(&edited(project, edit))
        .expect("a write to memory");
    zip.finish().expect("an archive").into_inner()
}

/// Puts `data` in the stream at `path`, in place of what it holds.
fn replace_stream(compound: &mut Compound, path: &str, data: &[u8]) {
    compound.remove_stream(path).expect("the stream is there");
    let mut stream = compound.create_stream(path).expect("a stream");
    stream.write_all(data).expect("a write to memory");
}

#[test]
fn a_project_without_its_dir_stream_is_corrupted_and_not_described() {
    let package = package(|c| c.remove_stream("/VBA/dir").expect("the stream is there"));

    let report = extraction(&package, "a.xlsm").file;

    assert_eq!(report.status, Status::Corrupted);
    assert!(report.vba_project.is_none());
    assert_eq!(codes(&report), ["ovba.dir.missing"]);
}

#[test]
fn a_module_without_its_stream_is_listed_without_source() {
    let package = package(|c| {
        c.remove_stream("/VBA/M_01_01")
            .expect("the stream is there");
    });

    let extraction = extraction(&package, "a.xlsm");

    let report = &extraction.file;
    assert_eq!(report.status, Status::Corrupted);
    assert_eq!(codes(report), ["ovba.module.missing_stream"]);
    let errors = report.errors.iter().flatten();
    let messages: Vec<_> = errors.map(|error| error.message.as_str()).collect();
    assert!(messages[0].starts_with("module M_01_01: "), "{messages:?}");
    let project = report.vba_project.as_ref().expect("a described project");
    let lost: Vec<_> = (project.modules.iter())
        .filter(|module| !module.source_available)
        .map(|module| (module.name.as_str(), &module.source_sha256))
        .collect();
    assert_eq!(lost, [("M_01_01", &None)]);
    assert_eq!(extraction.sources.iter().flatten().count(), 2);
}

#[test]
fn a_project_stream_above_the_limit_is_too_large_and_the_modules_are_still_read() {
    let big = vec![b' '; MAX_PROJECT_STREAM + 1];
    let package = package(|c| replace_stream(c, "/PROJECT", &big));

    let extraction = extraction(&package, "a.xlsm");

    assert_eq!(extraction.file.status, Status::TooLarge);
    assert_eq!(codes(&extraction.file), ["ovba.project.too_large"]);
    assert_eq!(extraction.sources.iter().flatten().count(), 3);
}

/// The stream of a module with `compiled` bytes of compiled part, whose source
/// decompresses to a little more than the 8 MiB that a module may take: 2,049
/// chunks of 4,096 bytes, each a literal and a copy of 4,095 bytes of it.
fn bomb(compiled: usize) -> Vec<u8> {
    let chunk = [0x03, 0xB0, 0x02, b'a', 0xFC, 0x0F];
    let mut stream = vec![0; compiled];
    stream.push(0x01);
    stream.extend(chunk.repeat(2049));
    stream
}

#[test]
fn the_modules_after_one_that_decompresses_past_its_limit_are_not_read() {
    let package = package(|c| {
        let mut dir = Vec::new();
        let mut stream = c.open_stream("/VBA/dir").expect("the stream is there");
        stream.read_to_end(&mut dir).expect("a read from memory");
        drop(stream);
        for module in parse_compressed_dir(&dir).expect("a dir stream").modules {
            let compiled = usize::try_from(module.text_offset).expect("an offset");
            replace_stream(c, &format!("/VBA/{}", module.stream_name), &bomb(compiled));
        }
    });

    let report = extraction(&package, "a.xlsm").file;

    assert_eq!(report.status, Status::SuspiciousArchive);
    let expected = [
        "ovba.decompress.too_large",
        "ovba.module.not_read",
        "ovba.module.not_read",
    ];
    assert_eq!(codes(&report), expected);
    let project = report.vba_project.as_ref().expect("a described project");
    assert_eq!(project.modules.len(), 3);
    assert!(
        project
            .modules
            .iter()
            .all(|module| !module.source_available)
    );
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
