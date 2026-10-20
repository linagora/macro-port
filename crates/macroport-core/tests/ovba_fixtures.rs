// SPDX-License-Identifier: Apache-2.0
//! The dir streams that Excel wrote, read from the `.xls` fixtures: the
//! project information and the module list agree with `fixtures.yaml`, and the
//! source of each module decompresses from the offset that the dir stream
//! gives. A1, which compares the decoded sources with Excel's own export,
//! belongs to the ticket that decodes them.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Read};

use cfb::CompoundFile;
use macroport_core::ovba::{DirInfo, decompress, parse_compressed_dir};
use macroport_testkit::corpus;
use serde::Deserialize;

/// The storage of the VBA project in a BIFF8 workbook.
const VBA_STORAGE: &str = "/_VBA_PROJECT_CUR/VBA";

/// Room for the source of a module of the fixtures, which are small.
const MAX_SOURCE: usize = 1 << 20;

/// The keys of `fixtures.yaml` that these tests use.
#[derive(Deserialize)]
struct Fixtures {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    file: String,
    code_page: Option<u16>,
    #[serde(default)]
    modules: Vec<ExpectedModule>,
    expected_module: Option<String>,
}

#[derive(Deserialize)]
struct ExpectedModule {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

/// The `.xls` fixtures with a VBA project to check: those that list their
/// modules, and the workbook with an open password, which names one.
fn xls_fixtures() -> Vec<Fixture> {
    let text = String::from_utf8(corpus::read("fixtures/fixtures.yaml")).unwrap();
    // The file starts with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let fixtures: Fixtures = serde_saphyr::from_str(text).unwrap();
    fixtures
        .fixtures
        .into_iter()
        .filter(|fixture| {
            fixture.file.ends_with(".xls")
                && (!fixture.modules.is_empty() || fixture.expected_module.is_some())
        })
        .collect()
}

/// A workbook of the fixtures, open as a compound file, with the dir stream
/// of its VBA project read.
struct Project {
    compound: CompoundFile<Cursor<Vec<u8>>>,
    dir: DirInfo,
}

impl Project {
    fn open(file: &str) -> Self {
        let bytes = corpus::read(&format!("fixtures/{file}"));
        let mut compound = CompoundFile::open(Cursor::new(bytes)).unwrap();
        let dir = parse_compressed_dir(&read_stream(&mut compound, "dir")).unwrap();
        Self { compound, dir }
    }

    /// The bytes of a stream of the VBA storage: `dir` or a module's.
    fn stream(&mut self, name: &str) -> Vec<u8> {
        read_stream(&mut self.compound, name)
    }
}

fn read_stream(compound: &mut CompoundFile<Cursor<Vec<u8>>>, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    compound
        .open_stream(format!("{VBA_STORAGE}/{name}"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn reads_the_project_information_of_every_xls_fixture() {
    let fixtures = xls_fixtures();
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        let dir = Project::open(&fixture.file).dir;
        if let Some(code_page) = fixture.code_page {
            assert_eq!(dir.code_page, Some(code_page), "{}", fixture.file);
        }
        // Build-Fixtures.ps1 leaves the default name of Excel.
        assert_eq!(
            dir.project_name.as_deref(),
            Some("VBAProject"),
            "{}",
            fixture.file
        );
    }
}

#[test]
fn reads_the_modules_of_every_xls_fixture() {
    for fixture in xls_fixtures() {
        let dir = Project::open(&fixture.file).dir;
        let mut names: Vec<_> = dir.modules.iter().map(|module| &module.name).collect();
        names.sort();
        let mut expected: Vec<_> = fixture.modules.iter().map(|module| &module.name).collect();
        expected.sort();
        if !expected.is_empty() {
            assert_eq!(names, expected, "{}", fixture.file);
        }
        if let Some(name) = &fixture.expected_module {
            assert!(names.contains(&name), "{}: {name}", fixture.file);
        }
        for expected in &fixture.modules {
            let module = dir
                .modules
                .iter()
                .find(|module| module.name == expected.name)
                .unwrap();
            // Only a standard module is procedural: documents, classes and
            // forms are not.
            assert_eq!(
                module.procedural,
                expected.kind == "standard",
                "{}: {}",
                fixture.file,
                module.name
            );
            assert_eq!(module.stream_name, module.name, "{}", fixture.file);
        }
    }
}

#[test]
fn keeps_no_path_in_the_references_of_any_xls_fixture() {
    for fixture in xls_fixtures() {
        let dir = Project::open(&fixture.file).dir;
        // Excel always references its own libraries.
        assert!(!dir.references.is_empty(), "{}", fixture.file);
        for reference in &dir.references {
            assert!(
                !reference.contains(['\\', '/']),
                "{}: {reference}",
                fixture.file
            );
        }
    }
}

#[test]
fn decompresses_the_source_of_every_module_of_every_xls_fixture() {
    for fixture in xls_fixtures() {
        let mut project = Project::open(&fixture.file);
        for module in project.dir.modules.clone() {
            let stream = project.stream(&module.stream_name);
            let offset = usize::try_from(module.text_offset).unwrap();
            let source = decompress(stream.get(offset..).unwrap(), MAX_SOURCE).unwrap();
            // The source of every module opens with the attribute that names it.
            assert!(
                source.starts_with(format!("Attribute VB_Name = \"{}\"", module.name).as_bytes()),
                "{}: {}",
                fixture.file,
                module.name
            );
        }
    }
}

#[test]
fn never_panics_on_a_damaged_dir_stream() {
    let fixture = xls_fixtures().into_iter().next().unwrap();
    let dir = Project::open(&fixture.file).stream("dir");
    // Every prefix of the stream, then every byte altered in three ways.
    for length in 0..dir.len() {
        let _ = parse_compressed_dir(dir.get(..length).unwrap());
    }
    for index in 0..dir.len() {
        for mask in [0x01, 0x80, 0xFF] {
            let mut damaged = dir.clone();
            *damaged.get_mut(index).unwrap() ^= mask;
            let _ = parse_compressed_dir(&damaged);
        }
    }
}
