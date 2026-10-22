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
use macroport_core::container::{MAX_STREAM_SIZE, read_ooxml};
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
/// is `count` chunks of 4,096 bytes, each a literal and a copy of 4,095 bytes of
/// it, then `tail`.
fn stream_of_chunks(compiled: usize, count: usize, tail: &[u8]) -> Vec<u8> {
    let chunk = [0x03, 0xB0, 0x02, b'a', 0xFC, 0x0F];
    let mut stream = vec![0; compiled];
    stream.push(0x01);
    stream.extend(chunk.repeat(count));
    stream.extend_from_slice(tail);
    stream
}

/// The stream of a module whose source decompresses to a little more than the
/// 8 MiB that a module may take: 2,049 chunks.
fn bomb(compiled: usize) -> Vec<u8> {
    stream_of_chunks(compiled, 2049, &[])
}

/// The stream of a module whose source decompresses to the 8 MiB that a module
/// may take, and then breaks: 2,048 chunks, and a chunk header without a
/// signature.
fn bomb_that_breaks(compiled: usize) -> Vec<u8> {
    stream_of_chunks(compiled, 2048, &[0x00, 0x00])
}

/// A package whose modules all have the stream that `stream` makes from the
/// length of their compiled part.
fn package_of_streams(stream: fn(usize) -> Vec<u8>) -> Vec<u8> {
    package(|c| {
        for module in modules(c, "") {
            let compiled = usize::try_from(module.text_offset).expect("an offset");
            replace_stream(
                c,
                &format!("/VBA/{}", module.stream_name),
                &stream(compiled),
            );
        }
    })
}

#[test]
fn the_modules_after_one_that_decompresses_past_its_limit_are_not_read() {
    let package = package_of_streams(bomb);

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
fn the_modules_that_break_after_much_output_take_the_budget_of_the_project() {
    let package = package_of_streams(bomb_that_breaks);

    let extraction = extraction(&package, "a.xlsm");

    // The first two modules decompress 8 MiB each, as much as one may, and break.
    // They have taken 16 MiB of the 32 MiB of the project, so the third may
    // decompress 5.3 MiB only: it crosses that limit before the break. If the
    // reads that fail were free, the third would break like the others, and so
    // would any number of modules that name such a stream.
    let report = &extraction.file;
    assert_eq!(report.status, Status::Corrupted);
    let expected = [
        "ovba.decompress.bad_chunk",
        "ovba.decompress.bad_chunk",
        "ovba.decompress.too_large",
    ];
    assert_eq!(codes(report), expected);
    assert_eq!(extraction.indicators, ["sec.suspicious-archive"]);
}

/// A `dir` stream of `count` procedural modules, `M0`, `M1` and so on, that all
/// have the stream `stream` and no compiled part. It is compressed the way a
/// stream can be without a compressor: in raw chunks.
fn dir_of_modules_with_one_stream(count: usize, stream: &str) -> Vec<u8> {
    const MODULE_NAME: u16 = 0x0019;
    const MODULE_STREAM_NAME: u16 = 0x001A;
    const MODULE_OFFSET: u16 = 0x0031;
    const MODULE_PROCEDURAL: u16 = 0x0021;
    const MODULE_TERMINATOR: u16 = 0x002B;
    const TERMINATOR: u16 = 0x0010;
    let record = |id: u16, data: &[u8]| {
        let size = u32::try_from(data.len()).expect("a short record");
        [id.to_le_bytes().as_slice(), &size.to_le_bytes(), data].concat()
    };
    let mut records = Vec::new();
    for index in 0..count {
        records.extend(record(MODULE_NAME, format!("M{index}").as_bytes()));
        records.extend(record(MODULE_STREAM_NAME, stream.as_bytes()));
        records.extend(record(MODULE_OFFSET, &0_u32.to_le_bytes()));
        records.extend(record(MODULE_PROCEDURAL, b""));
        records.extend(record(MODULE_TERMINATOR, b""));
    }
    records.extend(record(TERMINATOR, b""));

    let mut container = vec![0x01];
    for chunk in records.chunks(4096) {
        container.extend([0xFF, 0x3F]);
        container.extend_from_slice(chunk);
        container.resize(container.len() + 4096 - chunk.len(), 0);
    }
    container
}

/// `bytes`, a compound file of 512-byte sectors, in which the directory says
/// that the stream `name` is `len` bytes long. The sectors are left as they are.
fn claiming(mut bytes: Vec<u8>, name: &str, len: u64) -> Vec<u8> {
    // A directory entry takes 128 bytes, from a multiple of 128: its name, in
    // UTF-16 with a null at the end, in the first 64, the length of that name
    // in bytes at 64, and the size of the stream at 120.
    let name: Vec<u8> = (name.encode_utf16().chain([0]))
        .flat_map(u16::to_le_bytes)
        .collect();
    let name_len = u16::try_from(name.len())
        .expect("a short name")
        .to_le_bytes();
    let entries: Vec<usize> = (bytes.as_chunks::<128>().0.iter().enumerate())
        .filter(|(_, entry)| entry.starts_with(&name) && entry.get(64..66) == Some(&name_len[..]))
        .map(|(index, _)| index * 128)
        .collect();
    assert_eq!(entries.len(), 1, "one entry has the name of the stream");
    let size_at = entries.first().expect("an entry") + 120;
    let size = bytes.get_mut(size_at..size_at + 8).expect("a size");
    size.copy_from_slice(&len.to_le_bytes());
    bytes
}

/// The `.xls` of the workbook, with a project of `count` modules that all name
/// one stream. The stream holds 5,000 bytes, and the directory says that it
/// holds `claimed`.
fn workbook_with_modules_on_one_stream(count: usize, claimed: u64) -> Vec<u8> {
    const STREAM: &str = "M_01_01";
    let workbook = workbook(|c| {
        let dir = dir_of_modules_with_one_stream(count, STREAM);
        replace_stream(c, "/_VBA_PROJECT_CUR/VBA/dir", &dir);
        replace_stream(c, &format!("/_VBA_PROJECT_CUR/VBA/{STREAM}"), &[b' '; 5000]);
    });
    claiming(workbook, STREAM, claimed)
}

#[test]
fn the_reads_of_a_stream_that_many_modules_name_stop_at_the_limit_of_the_project() {
    // Every module names the same stream. The directory says that it is 64 MiB,
    // the longest that is read, and its chain holds 5,000 bytes, so each read
    // fails after a small copy. The reads are counted at what the directory
    // says: three make 192 MiB, under the limit of 200 MiB, and the fourth
    // would make 256 MiB. Counted at what they copy, they would never add up
    // to the limit.
    let xls = workbook_with_modules_on_one_stream(6, MAX_STREAM_SIZE);

    let extraction = extraction(&xls, "a.xls");

    let report = &extraction.file;
    let expected = [
        "cfb.truncated",
        "cfb.truncated",
        "cfb.truncated",
        "ovba.project.streams_too_large",
        "ovba.module.not_read",
        "ovba.module.not_read",
    ];
    assert_eq!(codes(report), expected);
    // The status is that of the first failure. The suspicious one is the
    // project's, and is raised all the same.
    assert_eq!(report.status, Status::Corrupted);
    assert_eq!(extraction.indicators, ["sec.suspicious-archive"]);
    let project = report.vba_project.as_ref().expect("a described project");
    assert_eq!(project.modules.len(), 6);
    assert!(extraction.sources.iter().all(Option::is_none));
}

#[test]
fn a_stream_above_the_size_limit_is_refused_by_the_container_and_costs_the_project_nothing() {
    // The container refuses these streams before it reads a byte, and says
    // `too_large`. Were they counted, the fourth would make the project
    // suspicious instead.
    let xls = workbook_with_modules_on_one_stream(6, MAX_STREAM_SIZE + 1);

    let extraction = extraction(&xls, "a.xls");

    let report = &extraction.file;
    assert_eq!(codes(report), ["cfb.stream_too_large"; 6]);
    assert_eq!(report.status, Status::TooLarge);
    assert!(extraction.indicators.is_empty());
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
        let signal = file_signal("sec.source-missing", Severity::High);
        assert_eq!(report.signals, Some(vec![signal]), "{path}");
        assert_eq!(report.max_severity, Some(Severity::High), "{path}");
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
        // The signals are those that extraction made from the indicators.
        let signal = file_signal("sec.source-missing", Severity::High);
        assert_eq!(report.signals, Some(vec![signal]), "{path}");
        assert_eq!(report.max_severity, Some(Severity::High), "{path}");
    }
}
