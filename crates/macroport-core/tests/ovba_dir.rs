// SPDX-License-Identifier: Apache-2.0
//! The `dir` stream: the records of the MS-OVBA examples, the quirks that it
//! tolerates, its limits and hostile input.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

mod ovba_common;

use macroport_core::ovba::{
    DirInfo, DirModule, MAX_DIR_STREAM, MAX_MODULES, MAX_REFERENCES, parse_compressed_dir,
    parse_dir,
};
use macroport_core::report::Status;
use ovba_common::Lcg;

// Builders for the records of the `dir` stream.

/// A length as 4 bytes.
fn size(data: &[u8]) -> [u8; 4] {
    u32::try_from(data.len()).unwrap().to_le_bytes()
}

/// A record: its id, its size and its data.
fn record(id: u16, data: &[u8]) -> Vec<u8> {
    [id.to_le_bytes().as_slice(), &size(data), data].concat()
}

/// A field made of its length and its bytes.
fn length_prefixed(data: &[u8]) -> Vec<u8> {
    [size(data).as_slice(), data].concat()
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

const PROCEDURAL: u16 = 0x0021;
const NON_PROCEDURAL: u16 = 0x0022;

/// A module record in the way of the examples of the specification.
fn module(name: &str, text_offset: u32, type_id: u16) -> Vec<u8> {
    module_with(name, text_offset, type_id, &[])
}

/// A module record with more records between its type and its terminator.
fn module_with(name: &str, text_offset: u32, type_id: u16, more: &[Vec<u8>]) -> Vec<u8> {
    [
        record(0x0019, name.as_bytes()),
        record(0x0047, &utf16(name)),
        record(0x001A, name.as_bytes()),
        record(0x0032, &utf16(name)),
        record(0x001C, b""),
        record(0x0048, b""),
        record(0x0031, &text_offset.to_le_bytes()),
        record(0x001E, &0_u32.to_le_bytes()),
        record(0x002C, &0xFFFF_u16.to_le_bytes()),
        record(type_id, b""),
        more.concat(),
        record(0x002B, b""),
    ]
    .concat()
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

/// A REFERENCENAME record.
fn name_record(name: &str) -> Vec<u8> {
    [
        record(0x0016, name.as_bytes()),
        record(0x003E, &utf16(name)),
    ]
    .concat()
}

/// A reference record: its id, then the size that a reader ignores, then its
/// fields.
fn reference(id: u16, fields: &[Vec<u8>]) -> Vec<u8> {
    let fields = fields.concat();
    [id.to_le_bytes().as_slice(), &size(&fields), &fields].concat()
}

fn registered(libid: &str) -> Vec<u8> {
    reference(0x000D, &[length_prefixed(libid.as_bytes()), vec![0; 6]])
}

fn project_reference(absolute: &str, relative: &str) -> Vec<u8> {
    reference(
        0x000E,
        &[
            length_prefixed(absolute.as_bytes()),
            length_prefixed(relative.as_bytes()),
            vec![0x46, 0x5F, 0xA9, 0x49, 0x0D, 0x00],
        ],
    )
}

/// A control reference: the twiddled libid, the name of the extended type
/// library when there is one, then the extended libid.
fn control(twiddled: &str, name: Option<&str>, extended: &str) -> Vec<u8> {
    let first = [length_prefixed(twiddled.as_bytes()), vec![0; 6]].concat();
    let second = [length_prefixed(extended.as_bytes()), vec![0; 26]].concat();
    [
        0x002F_u16.to_le_bytes().as_slice(),
        &size(&first),
        &first,
        &name.map(name_record).unwrap_or_default(),
        &0x0030_u16.to_le_bytes(),
        &size(&second),
        &second,
    ]
    .concat()
}

const OLE_AUTOMATION: &str = r"*\G{00020430-0000-0000-C000-000000000046}#2.0#0#C:\Windows\system32\stdole2.tlb#OLE Automation";
const OFFICE_12: &str = r"*\G{2DF8D04C-5BFA-101B-BDE5-00AA0044DE52}#2.0#0#C:\Program Files\Common Files\Microsoft Shared\OFFICE12\MSO.DLL#Microsoft Office 12.0 Object Library";
const DUMMY_LIBID: &str = r"*\G{00000000-0000-0000-0000-000000000000}#0.0#0##";
const MS_FORMS: &str = r"*\G{896C2D83-5466-46ED-8FAE-4C3E4F85E710}#2.0#0#C:\Users\jsmith\AppData\Local\Temp\VBE\MSForms.exd#Microsoft Forms 2.0 Object Library";

/// The four references of the specification example.
fn example_references() -> Vec<Vec<u8>> {
    vec![
        name_record("stdole"),
        registered(OLE_AUTOMATION),
        name_record("Office"),
        registered(OFFICE_12),
        name_record("VBAProject1"),
        project_reference(
            r"*\CC:\Example Path\Example-ReferencedProject.xls",
            r"*\CExample-ReferencedProject.xls",
        ),
        name_record("MSForms"),
        control(DUMMY_LIBID, Some("MSForms"), MS_FORMS),
    ]
}

fn dir_module(name: &str, text_offset: u32, procedural: bool) -> DirModule {
    DirModule {
        name: name.to_owned(),
        stream_name: name.to_owned(),
        text_offset,
        procedural,
    }
}

/// A project with information, references and modules.
fn example_stream() -> Vec<u8> {
    dir(
        &information_records(1252),
        &example_references(),
        &[
            module("ThisWorkbook", 0, NON_PROCEDURAL),
            module("Module1", 815, PROCEDURAL),
        ],
    )
}

// The examples of the specification.

#[test]
fn reads_the_project_information_of_the_specification_example() {
    let info = parse(&information_records(0x04E4), &[], &[]);
    assert_eq!(info.code_page, Some(1252));
    assert_eq!(info.project_name.as_deref(), Some("VBAProject"));
    assert!(info.references.is_empty());
    assert!(info.modules.is_empty());
}

#[test]
fn reads_the_references_of_the_specification_example() {
    let info = parse(&information_records(1252), &example_references(), &[]);
    assert_eq!(
        info.references,
        [
            "OLE Automation",
            "Microsoft Office 12.0 Object Library",
            "VBAProject1",
            "Microsoft Forms 2.0 Object Library",
        ]
    );
}

#[test]
fn reads_the_module_records_of_the_specification_examples() {
    let modules = [
        module("ThisWorkbook", 0, NON_PROCEDURAL),
        // The designer module of the specification is private.
        module_with("UserForm1", 0, NON_PROCEDURAL, &[record(0x0028, b"")]),
        module("Module1", 815, PROCEDURAL),
    ];
    let info = parse(&information_records(1252), &[], &modules);
    assert_eq!(
        info.modules,
        [
            dir_module("ThisWorkbook", 0, false),
            dir_module("UserForm1", 0, false),
            dir_module("Module1", 815, true),
        ]
    );
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

// Module names.

#[test]
fn prefers_the_unicode_names() {
    let module = [
        record(0x0019, b"Old"),
        record(0x0047, &utf16("Caf\u{e9}")),
        record(0x001A, b"OldStream"),
        record(0x0032, &utf16("Caf\u{e9}Stream")),
        record(0x0021, b""),
        record(0x002B, b""),
    ]
    .concat();
    let info = parse(&information_records(1252), &[], &[module]);
    assert_eq!(info.modules.len(), 1);
    assert_eq!(info.modules[0].name, "Caf\u{e9}");
    assert_eq!(info.modules[0].stream_name, "Caf\u{e9}Stream");
}

#[test]
fn decodes_the_mbcs_names_in_the_code_page_when_there_is_no_unicode_name() {
    let cases: [(u16, &[u8], &str); 3] = [
        (1252, &[0x43, 0x61, 0x66, 0xE9], "Caf\u{e9}"),
        (1251, &[0xCF, 0xF0, 0xE8], "\u{41f}\u{440}\u{438}"),
        (932, &[0x93, 0xFA, 0x96, 0x7B], "\u{65e5}\u{672c}"),
    ];
    for (code_page, name, expected) in cases {
        let module = [
            record(0x0019, name),
            record(0x001A, name),
            record(0x0031, &9_u32.to_le_bytes()),
            record(0x0021, b""),
            record(0x002B, b""),
        ]
        .concat();
        let info = parse(&information_records(code_page), &[], &[module]);
        assert_eq!(info.modules, [dir_module(expected, 9, true)]);
    }
}

#[test]
fn an_unknown_code_page_decodes_the_mbcs_names_as_windows_1252() {
    for code_page in [0, 12345] {
        let module = [record(0x0019, &[0x43, 0xE9]), record(0x002B, b"")].concat();
        let info = parse(&information_records(code_page), &[], &[module]);
        assert_eq!(info.code_page, Some(code_page));
        assert_eq!(info.modules[0].name, "C\u{e9}");
    }
}

#[test]
fn a_module_without_a_name_takes_the_name_of_its_stream_and_the_other_way_round() {
    let nameless = [
        record(0x0019, b""),
        record(0x001A, b"Stream1"),
        record(0x002B, b""),
    ]
    .concat();
    let streamless = [record(0x0019, b"Module2"), record(0x002B, b"")].concat();
    let info = parse(&information_records(1252), &[], &[nameless, streamless]);
    assert_eq!(
        info.modules,
        [
            dir_module("Stream1", 0, false),
            dir_module("Module2", 0, false)
        ]
    );
}

#[test]
fn drops_a_module_that_has_no_name_at_all() {
    let module = [
        record(0x0019, b""),
        record(0x0031, &5_u32.to_le_bytes()),
        record(0x002B, b""),
    ]
    .concat();
    let info = parse(&information_records(1252), &[], &[module]);
    assert!(info.modules.is_empty());
}

#[test]
fn a_module_record_outside_a_module_belongs_to_no_module() {
    // An offset and a type come before any module name, and again past the
    // terminator of the first module.
    let stray = [record(0x0031, &99_u32.to_le_bytes()), record(0x0021, b"")].concat();
    let records = [information_records(1252), vec![stray.clone()]].concat();
    let first = [module("A", 4, NON_PROCEDURAL), stray].concat();
    let info = parse(&records, &[], &[first, module("B", 5, NON_PROCEDURAL)]);
    assert_eq!(
        info.modules,
        [dir_module("A", 4, false), dir_module("B", 5, false)]
    );
}

#[test]
fn a_module_ends_at_the_next_module_name_or_the_dir_terminator() {
    // No module terminators.
    let stream = [
        information_records(1252).concat(),
        record(0x0019, b"A"),
        record(0x0031, &1_u32.to_le_bytes()),
        record(0x0021, b""),
        record(0x0019, b"B"),
        record(0x0031, &2_u32.to_le_bytes()),
        record(0x0010, b""),
    ]
    .concat();
    let info = parse_dir(&stream).unwrap();
    assert_eq!(
        info.modules,
        [dir_module("A", 1, true), dir_module("B", 2, false)]
    );
}

#[test]
fn ignores_the_reserved_field_of_the_module_records_that_have_no_data() {
    // The specification gives them a reserved field, ignored on read, where
    // the others have a size. A reader that took it for a size would skip
    // 0xEEEEEEEE bytes.
    let reserved = |id: u16| [id.to_le_bytes().as_slice(), &[0xEE; 4]].concat();
    let named = |name: &str| {
        [
            record(0x0019, name.as_bytes()),
            reserved(0x0021),
            reserved(0x0025),
            reserved(0x0028),
            reserved(0x002B),
        ]
        .concat()
    };
    let info = parse(&information_records(1252), &[], &[named("A"), named("B")]);
    assert_eq!(
        info.modules,
        [dir_module("A", 0, true), dir_module("B", 0, true)]
    );
}

// References.

#[test]
fn ignores_the_size_of_a_reference_record() {
    let mut references = example_references();
    for index in [1, 3, 5, 7] {
        // A reader ignores it, so a writer may leave anything there.
        for byte in references[index].get_mut(2..6).unwrap() {
            *byte = 0xEE;
        }
    }
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references.len(), 4);
    assert_eq!(info.references[3], "Microsoft Forms 2.0 Object Library");
}

#[test]
fn reads_a_control_reference_that_has_no_extended_name() {
    let references = [control(DUMMY_LIBID, None, MS_FORMS)];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["Microsoft Forms 2.0 Object Library"]);
}

#[test]
fn a_control_reference_prefers_the_extended_libid_to_the_twiddled_one() {
    let references = [control(OLE_AUTOMATION, None, MS_FORMS)];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["Microsoft Forms 2.0 Object Library"]);
}

#[test]
fn reads_a_control_reference_that_follows_an_original_reference() {
    // REFERENCEORIGINAL sits between the name and the control record.
    let original = record(0x0033, &length_prefixed(OLE_AUTOMATION.as_bytes()));
    let references = [
        name_record("MSForms"),
        original,
        control(DUMMY_LIBID, Some("MSForms"), MS_FORMS),
    ];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["Microsoft Forms 2.0 Object Library"]);
}

#[test]
fn a_control_reference_with_a_dummy_libid_and_no_extended_one_falls_back_to_its_name() {
    let references = [control(DUMMY_LIBID, Some("MSForms"), DUMMY_LIBID)];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["MSForms"]);
}

#[test]
fn a_reference_without_a_usable_libid_is_named_after_its_name_record() {
    let references = [
        name_record("Scripting"),
        registered("not a libid"),
        name_record("Another"),
        registered(r"*\G{00020430-0000-0000-C000-000000000046}#2.0#0#only-four-fields"),
        // No name record at all: nothing to show.
        registered("still not a libid"),
    ];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["Scripting", "Another"]);
}

#[test]
fn reads_the_registered_name_of_a_macintosh_library() {
    // The kind of its path is H, where that of a Windows library is G.
    let libid =
        r"*\H{00020430-0000-0000-C000-000000000046}#2.0#0#Macintosh HD:stdole2.tlb#OLE Automation";
    let info = parse(&information_records(1252), &[registered(libid)], &[]);
    assert_eq!(info.references, ["OLE Automation"]);
}

#[test]
fn a_registered_name_may_contain_a_hash() {
    let references = [registered(
        r"*\G{00020430-0000-0000-C000-000000000046}#2.0#0#C:\x.tlb#Library #2",
    )];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(info.references, ["Library #2"]);
}

#[test]
fn a_reference_never_shows_a_path() {
    let paths = [
        r"C:\Users\bob\secret.tlb",
        r"\\server\share\secret.tlb",
        "/home/bob/secret.tlb",
        "d:secret",
        r"..\..\secret",
        "bob/secret",
    ];
    for path in paths {
        let libid = format!(r"*\G{{00020430-0000-0000-C000-000000000046}}#2.0#0#C:\x.tlb#{path}");
        let references = [
            // The registered name is a path: the name record stands in.
            name_record("Safe"),
            registered(&libid),
            // Both are paths: the reference is dropped.
            name_record(path),
            registered(&libid),
            // A project reference shows its name, never its paths.
            name_record(path),
            project_reference(r"*\CC:\Users\bob\other.xlsm", r"*\Cother.xlsm"),
            name_record("Other"),
            project_reference(r"*\CC:\Users\bob\other.xlsm", r"*\Cother.xlsm"),
        ];
        let info = parse(&information_records(1252), &references, &[]);
        assert_eq!(info.references, ["Safe", "Other"], "path {path}");
    }
}

#[test]
fn a_reference_never_shows_the_path_of_its_libid() {
    let references = example_references();
    let info = parse(&information_records(1252), &references, &[]);
    for name in &info.references {
        assert!(!name.contains(['\\', '/', ':']), "{name}");
    }
}

#[test]
fn keeps_the_references_in_the_order_of_the_stream() {
    let references = [
        registered(OFFICE_12),
        registered(OLE_AUTOMATION),
        registered(OFFICE_12),
    ];
    let info = parse(&information_records(1252), &references, &[]);
    assert_eq!(
        info.references,
        [
            "Microsoft Office 12.0 Object Library",
            "OLE Automation",
            "Microsoft Office 12.0 Object Library"
        ]
    );
}

// Limits.

fn many_modules(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|index| {
            [
                record(0x0019, format!("M{index}").as_bytes()),
                record(0x002B, b""),
            ]
            .concat()
        })
        .collect()
}

#[test]
fn accepts_the_most_modules() {
    let info = parse(&information_records(1252), &[], &many_modules(MAX_MODULES));
    assert_eq!(info.modules.len(), MAX_MODULES);
    assert_eq!(info.modules[MAX_MODULES - 1].name, "M1999");
}

#[test]
fn rejects_one_module_too_many() {
    let stream = dir(
        &information_records(1252),
        &[],
        &many_modules(MAX_MODULES + 1),
    );
    let error = parse_dir(&stream).unwrap_err();
    assert_eq!(error.status, Status::TooLarge);
    assert_eq!(error.code, "ovba.dir.too_many_modules");
}

#[test]
fn counts_the_last_module_even_when_nothing_terminates_it() {
    let mut modules = many_modules(MAX_MODULES);
    modules.push(record(0x0019, b"Last"));
    let error = parse_dir(&dir(&information_records(1252), &[], &modules)).unwrap_err();
    assert_eq!(error.code, "ovba.dir.too_many_modules");
}

#[test]
fn accepts_the_most_references_and_rejects_one_more() {
    let references = |count: usize| vec![registered(OLE_AUTOMATION); count];
    let info = parse(&information_records(1252), &references(MAX_REFERENCES), &[]);
    assert_eq!(info.references.len(), MAX_REFERENCES);

    let stream = dir(
        &information_records(1252),
        &references(MAX_REFERENCES + 1),
        &[],
    );
    let error = parse_dir(&stream).unwrap_err();
    assert_eq!(error.status, Status::TooLarge);
    assert_eq!(error.code, "ovba.dir.too_many_references");
}

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
