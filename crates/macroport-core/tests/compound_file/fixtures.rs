// SPDX-License-Identifier: Apache-2.0
//! What Excel wrote: the `.xls` workbooks and the VBA project of an `.xlsm`.

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use macroport_core::container::{CfbFile, read_ooxml};
use macroport_testkit::corpus;

use crate::support::open;

/// The same macros saved by Excel as an `.xls` and as an `.xlsm`.
const WORKBOOK: &str = "fixtures/generated/01-01-hide-unhide-columns-find";

#[test]
fn the_xls_fixtures_open_and_hold_a_workbook_stream() {
    let names = corpus::files("fixtures/generated", &["xls"]);
    assert!(names.len() > 70, "{} workbooks", names.len());
    for name in names {
        let bytes = corpus::read(&name);
        let mut file = CfbFile::open(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));

        let workbook = file.read_stream("Workbook").unwrap();
        assert!(
            workbook.is_some_and(|workbook| workbook.len() > 100),
            "{name}"
        );
        // All the workbooks hold a VBA project, but the one with an XLM macro
        // sheet and no VBA.
        assert_eq!(
            file.has_storage("_VBA_PROJECT_CUR"),
            !name.ends_with("/xlm-01-macrosheet.xls"),
            "{name}"
        );
    }
}

/// The streams of a container, read by the crate itself: the reference for
/// what the reader must return.
pub fn reference_streams(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut file = CompoundFile::open(Cursor::new(bytes)).unwrap();
    let paths: Vec<String> = file
        .walk()
        .filter(cfb::Entry::is_stream)
        .map(|entry| entry.path().to_str().unwrap().to_owned())
        .collect();
    paths
        .into_iter()
        .map(|path| {
            let mut data = Vec::new();
            file.open_stream(&path)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            (path, data)
        })
        .collect()
}

/// Asserts that the reader returns every stream of the container, under its
/// path as written and in upper and lower case, and that `expected` are among
/// the streams.
fn assert_reads_every_stream(bytes: &[u8], expected: &[&str]) {
    let reference = reference_streams(bytes);
    let paths: Vec<&str> = reference.iter().map(|(path, _)| path.as_str()).collect();
    for path in expected {
        assert!(paths.contains(path), "{path} is not in {paths:?}");
    }

    let mut file = open(bytes);
    for (path, data) in &reference {
        let path = path.trim_start_matches('/');
        for spelling in [path.to_owned(), path.to_uppercase(), path.to_lowercase()] {
            assert_eq!(
                file.read_stream(&spelling).unwrap().as_ref(),
                Some(data),
                "{spelling}"
            );
        }
    }
}

#[test]
fn a_real_xls_reads_its_streams_by_case_insensitive_path() {
    let bytes = corpus::read(&format!("{WORKBOOK}.xls"));
    assert_reads_every_stream(&bytes, &["/Workbook", "/_VBA_PROJECT_CUR/VBA/dir"]);
}

#[test]
fn a_vba_project_bin_reads_its_streams_by_case_insensitive_path() {
    // The `xl/vbaProject.bin` part of the package: a container of its own,
    // which holds the VBA project at its root.
    let package = read_ooxml(&corpus::read(&format!("{WORKBOOK}.xlsm"))).unwrap();
    let bytes = package.vba_project.expect("the package holds a VBA part");
    assert_reads_every_stream(&bytes, &["/PROJECT", "/VBA/dir"]);
}
