// SPDX-License-Identifier: Apache-2.0
//! A workbook with a part taken out or changed: the report keeps what can be
//! read, and says why the rest cannot be, or that there was nothing to read.
//! The files are the fixtures of one workbook, with a stream or a storage of
//! a compound file taken out or put back changed: the `.xls` itself, or the
//! VBA project of the `.xlsm`, which a new package then holds.

// `clippy.toml` bans the core from writing compound files. These tests change
// the one of a fixture.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use cfb::CompoundFile;
use macroport_core::analyze::{Options, analyze};
use macroport_core::container::read_ooxml;
use macroport_core::ovba::{DirModule, MAX_PROJECT_STREAM, parse_compressed_dir};
use macroport_core::report::{Status, Xlm};
use macroport_core::rules::Severity;
use macroport_testkit::corpus;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::support::{RULES, codes, extraction, file_signal};

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

/// What the stream at `path` holds.
fn stream_of(compound: &mut Compound, path: &str) -> Vec<u8> {
    let mut data = Vec::new();
    let mut stream = compound.open_stream(path).expect("the stream is there");
    stream.read_to_end(&mut data).expect("a read from memory");
    data
}

/// The modules that the `dir` stream of the project in the storage `storage`
/// names: empty for the project of a package, which is the whole compound file.
fn modules(compound: &mut Compound, storage: &str) -> Vec<DirModule> {
    let dir = stream_of(compound, &format!("{storage}/VBA/dir"));
    parse_compressed_dir(&dir).expect("a dir stream").modules
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
    // Without its source, whether it is missing is not known.
    let lost: Vec<_> = (project.modules.iter())
        .filter(|module| !module.source_available)
        .map(|module| {
            let missing = module.source_missing_suspected;
            (module.name.as_str(), &module.source_sha256, missing)
        })
        .collect();
    assert_eq!(lost, [("M_01_01", &None, None)]);
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
        for module in modules(c, "") {
            let compiled = usize::try_from(module.text_offset).expect("an offset");
            replace_stream(c, &format!("/VBA/{}", module.stream_name), &bomb(compiled));
        }
    });

    let extraction = extraction(&package, "a.xlsm");

    let report = &extraction.file;
    assert_eq!(report.status, Status::SuspiciousArchive);
    let expected = [
        "ovba.decompress.too_large",
        "ovba.module.not_read",
        "ovba.module.not_read",
    ];
    assert_eq!(codes(report), expected);
    assert_eq!(extraction.indicators, ["sec.suspicious-archive"]);
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

/// Takes the source out of the module `name` of the project in the storage
/// `storage`, as VBA stomping does: the compiled part stays, and the container
/// of the source holds nothing.
fn stomp(compound: &mut Compound, storage: &str, name: &str) {
    let module = (modules(compound, storage).into_iter())
        .find(|module| module.name == name)
        .expect("the module is there");
    let path = format!("{storage}/VBA/{}", module.stream_name);
    let mut stream = stream_of(compound, &path);
    stream.truncate(usize::try_from(module.text_offset).expect("an offset"));
    stream.push(0x01);
    replace_stream(compound, &path, &stream);
}

#[test]
fn a_module_whose_source_was_taken_out_is_suspected_of_having_lost_it() {
    let files = [
        ("a.xlsm", package(|c| stomp(c, "", "M_01_01"))),
        (
            "a.xls",
            workbook(|c| stomp(c, "/_VBA_PROJECT_CUR", "M_01_01")),
        ),
    ];
    for (path, bytes) in &files {
        let extraction = extraction(bytes, path);

        let report = &extraction.file;
        // No code is left in sight, but the procedures are compiled: the file
        // has macros.
        assert_eq!(report.status, Status::Ok, "{path}");
        assert_eq!(codes(report), Vec::<&str>::new(), "{path}");
        let project = report.vba_project.as_ref().expect("a described project");
        let suspected: BTreeMap<_, _> = (project.modules.iter())
            .map(|module| (module.name.as_str(), module.source_missing_suspected))
            .collect();
        let expected = BTreeMap::from([
            ("M_01_01", Some(true)),
            ("Sheet1", Some(false)),
            ("ThisWorkbook", Some(false)),
        ]);
        assert_eq!(suspected, expected, "{path}");
        assert_eq!(extraction.indicators, ["sec.source-missing"], "{path}");
        // The security stage makes the signal: extraction does not.
        assert_eq!(
            (&report.signals, report.max_severity),
            (&None, None),
            "{path}"
        );
    }
}

#[test]
fn a_file_whose_source_was_taken_out_is_ok_after_the_front_end_with_the_same_signal() {
    let files = [
        ("a.xlsm", package(|c| stomp(c, "", "M_01_01"))),
        (
            "a.xls",
            workbook(|c| stomp(c, "/_VBA_PROJECT_CUR", "M_01_01")),
        ),
    ];
    for (path, bytes) in &files {
        let report = analyze(bytes, path, &Options::new(&RULES));

        // The front end finds no procedure in the source that is left, and the
        // procedures that were compiled count all the same.
        let project = report.vba_project.as_ref().expect("a described project");
        let procedures = (project.modules.iter()).map(|module| module.procedures.len());
        assert_eq!(procedures.sum::<usize>(), 0, "{path}");
        assert_eq!(report.status, Status::Ok, "{path}");
        // The rule engine makes the signal of the indicator that extraction raised.
        let signal = file_signal("sec.source-missing", Severity::High);
        assert_eq!(report.signals, Some(vec![signal]), "{path}");
        assert_eq!(report.max_severity, Some(Severity::High), "{path}");
    }
}
