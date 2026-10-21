// SPDX-License-Identifier: Apache-2.0
//! The `PROJECT` stream of a VBA project: whether the project is protected,
//! and the type of its modules. The values of the protection come from the
//! `CMG`, `DPB` and `GC` of the MS-OVBA example, which the helper that writes
//! the data encryption reproduces before it builds the others. The streams that
//! Excel wrote are read by the fixture tests.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

mod ovba_common;

use macroport_core::ovba::{DirModule, MAX_MODULES, MAX_PROJECT_STREAM, parse_project};
use macroport_core::rules::ModuleType;
use ovba_common::Lcg;

// The examples of the specification.

/// The `CMG` of the example: 4 bytes of data, which are 0, and 3 bytes of
/// IgnoredEnc.
const EXAMPLE_CMG: &str = "0705D8E3D8EDDBF1DBF1DBF1DBF1";
/// The `DPB` of the example: one byte of data, which is 0x00.
const EXAMPLE_DPB: &str = "0E0CD1ECDFF4E7F5E7F5E7";
/// The `GC` of the example: one byte of data, which is 0xFF, so the project is
/// visible.
const EXAMPLE_GC: &str = "1517CAF1D6F9D7F9D706";

/// The key of the project of the example, which the encryption starts from.
const PROJECT_KEY: u8 = 0xDF;

/// The `PROJECT` stream of the MS-OVBA example (Figure 12), as the text of the
/// figure prints it. The table of the figure gives 0x27F bytes for the stream,
/// two more than that text holds. The `PROJECT` stream of
/// `01-01-hide-unhide-columns-find.xls` has the same two line endings after
/// its `GC` line as the text.
const EXAMPLE: &str = concat!(
    "ID=\"{917DED54-440B-4FD1-A5C1-74ACF261E600}\"\r\n",
    "Document=ThisWorkbook/&H00000000\r\n",
    "Document=Sheet1/&H00000000\r\n",
    "Package={AC9F2F90-E877-11CE-9F68-00AA00574A4F}\r\n",
    "BaseClass=UserForm1\r\n",
    "HelpFile=\"c:\\example path\\example.hlp\"\r\n",
    "Name=\"VBAProject\"\r\n",
    "HelpContextID=\"1\"\r\n",
    "Description=\"Example VBA Project Description\"\r\n",
    "VersionCompatible32=\"393222000\"\r\n",
    "CMG=\"0705D8E3D8EDDBF1DBF1DBF1DBF1\"\r\n",
    "DPB=\"0E0CD1ECDFF4E7F5E7F5E7\"\r\n",
    "GC=\"1517CAF1D6F9D7F9D706\"\r\n",
    "\r\n",
    "[Host Extender Info]\r\n",
    "&H00000001={3832D640-CF90-11CF-8E43-00A0C911005A};VBE;&H00000000\r\n",
    "\r\n",
    "[Workspace]\r\n",
    "ThisWorkbook=23, 23, 911, 280, \r\n",
    "Sheet1=69, 69, 724, 317, C\r\n",
    "UserForm1=0, 0, 0, 0, C, 46, 46, 701, 294, Z",
);

// Builders.

/// The version of the encryption that MS-OVBA defines.
const VERSION: u8 = 2;

/// The bytes of an Encrypted Data Structure (MS-OVBA, data encryption) of the
/// given version, which holds `data` and declares `length` bytes of it. The
/// bytes of IgnoredEnc are 0x07, as in the examples.
fn encrypted_with(seed: u8, project_key: u8, version: u8, length: u32, data: &[u8]) -> Vec<u8> {
    let version_enc = seed ^ version;
    let key_enc = seed ^ project_key;
    // Seed BAND 6, halved.
    let ignored = usize::from((seed & 6) / 2);
    let plain = std::iter::repeat_n(7, ignored)
        .chain(length.to_le_bytes())
        .chain(data.iter().copied());
    let mut bytes = vec![seed, version_enc, key_enc];
    let (mut previous, mut last, mut last_plain) = (version_enc, key_enc, project_key);
    for byte in plain {
        let cipher = byte ^ previous.wrapping_add(last_plain);
        (previous, last, last_plain) = (last, cipher, byte);
        bytes.push(cipher);
    }
    bytes
}

/// The bytes of an Encrypted Data Structure that holds `data`.
fn encrypted(seed: u8, project_key: u8, data: &[u8]) -> Vec<u8> {
    let length = u32::try_from(data.len()).unwrap();
    encrypted_with(seed, project_key, VERSION, length, data)
}

/// The structure that holds `data`, with the seed of the `DPB` of the example
/// and the key of its project, which most tests need no other than.
fn example_encrypted(data: &[u8]) -> Vec<u8> {
    encrypted(0x0E, PROJECT_KEY, data)
}

/// The same, with a version and a length that the test chooses.
fn example_encrypted_with(version: u8, length: u32, data: &[u8]) -> Vec<u8> {
    encrypted_with(0x0E, PROJECT_KEY, version, length, data)
}

/// Hexadecimal digits, as the `PROJECT` stream writes them.
fn hex_digits(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// A `GC` property whose value is `value`, as the `PROJECT` stream writes it,
/// without its line ending.
fn gc_line(value: &str) -> String {
    format!("GC=\"{value}\"")
}

/// A `PROJECT` stream whose `GC` property holds `value`.
fn with_gc(value: &str) -> String {
    format!("Name=\"VBAProject\"\r\n{}\r\n", gc_line(value))
}

fn protected(stream: &str) -> bool {
    parse_project(stream.as_bytes(), Some(1252)).protected
}

/// Whether a project whose `GC` holds these bytes is protected.
fn protected_bytes(bytes: &[u8]) -> bool {
    protected(&with_gc(&hex_digits(bytes)))
}

#[test]
fn the_helper_writes_the_examples_of_the_specification() {
    assert_eq!(
        hex_digits(&encrypted(0x07, PROJECT_KEY, &[0; 4])),
        EXAMPLE_CMG
    );
    assert_eq!(hex_digits(&example_encrypted(&[0x00])), EXAMPLE_DPB);
    assert_eq!(
        hex_digits(&encrypted(0x15, PROJECT_KEY, &[0xFF])),
        EXAMPLE_GC
    );
}

#[test]
fn reads_an_unprotected_project() {
    assert!(!parse_project(EXAMPLE.as_bytes(), Some(1252)).protected);
}

#[test]
fn reads_a_protected_project() {
    // The DPB of the example is the encryption of one byte, 0x00, which is a
    // value that a GC may hold: the stream of the example, but for its GC.
    let stream = EXAMPLE.replace(EXAMPLE_GC, EXAMPLE_DPB);
    assert_ne!(stream, EXAMPLE);
    assert!(protected(&stream));
}

// The Encrypted Data Structure.

#[test]
fn reads_the_protection_of_a_project_with_any_seed() {
    // The seed gives the number of bytes of IgnoredEnc, from 0 to 3.
    for seed in 0..=u8::MAX {
        for key in [0x00, 0x55, PROJECT_KEY, 0xFF] {
            assert!(
                protected_bytes(&encrypted(seed, key, &[0x00])),
                "seed {seed:#04x}, key {key:#04x}"
            );
            for data in [0x01, 0x7F, 0x80, 0xFF] {
                assert!(
                    !protected_bytes(&encrypted(seed, key, &[data])),
                    "seed {seed:#04x}, key {key:#04x}, data {data:#04x}"
                );
            }
        }
    }
}

#[test]
fn reads_the_digits_in_either_case() {
    assert!(protected(&with_gc(&EXAMPLE_DPB.to_ascii_lowercase())));
}

#[test]
fn reads_a_project_with_no_gc_as_unprotected() {
    assert!(!protected(""));
    assert!(!protected("Name=\"VBAProject\"\r\n"));
}

#[test]
fn takes_a_gc_that_is_not_one_byte_long_for_unprotected() {
    // The CMG of the example holds 4 bytes, and a GC holds exactly one.
    assert!(!protected(&with_gc(EXAMPLE_CMG)));
    assert!(!protected_bytes(&example_encrypted(&[])));
    assert!(!protected_bytes(&example_encrypted(&[0, 0])));
}

#[test]
fn takes_a_gc_whose_version_is_not_2_for_unprotected() {
    // Structures that are well formed, and encrypted as their version says.
    assert!(protected_bytes(&example_encrypted_with(
        VERSION,
        1,
        &[0x00]
    )));
    for version in [0, 1, 3, 0xFF] {
        let bytes = example_encrypted_with(version, 1, &[0x00]);
        assert!(!protected_bytes(&bytes), "version {version}");
    }
}

#[test]
fn takes_a_gc_whose_length_does_not_match_its_data_for_unprotected() {
    // A length that is not that of the data, however large: no allocation
    // follows it.
    for length in [0, 2, 3, 0xFF, 0x1_0000, u32::MAX] {
        let bytes = example_encrypted_with(VERSION, length, &[0x00]);
        assert!(!protected_bytes(&bytes), "length {length:#x}");
    }
    let valid = example_encrypted(&[0x00]);
    assert!(protected_bytes(&valid));
    // A byte too many.
    let mut longer = valid.clone();
    longer.push(0x00);
    assert!(!protected_bytes(&longer));
    // Bytes missing, from the data byte back to all of them.
    for length in 0..valid.len() {
        assert!(
            !protected_bytes(valid.get(..length).unwrap()),
            "{length} bytes"
        );
    }
}

#[test]
fn takes_a_gc_that_is_not_hexadecimal_for_unprotected() {
    for damaged in [
        // An odd number of digits, the last one of which would make no byte.
        "E0CD1ECDFF4E7F5E7F5E7",
        "0E0CD1ECDFF4E7F5E7F5E",
        "0E0CD1ECDFF4E7F5E7F5E7A",
        // A character that is no digit: a letter, a space, a sign that a
        // parser of numbers would take, and two bytes of a letter.
        "0E0CD1ECDFF4E7F5E7F5EG",
        "0E0CD1ECDFF4E7F5E7F5E ",
        "+E0CD1ECDFF4E7F5E7F5E7",
        "0E0CD1ECDFF4E7F5E7\u{e9}E7",
        "",
    ] {
        assert!(!protected(&with_gc(damaged)), "{damaged:?}");
    }
}

// The property.

#[test]
fn reads_a_gc_only_between_double_quotes() {
    let stream = |value: &str| format!("Name=\"VBAProject\"\r\nGC={value}\r\n");
    assert!(protected(&stream(&format!("\"{EXAMPLE_DPB}\""))));
    assert!(!protected(&stream(EXAMPLE_DPB)));
    assert!(!protected(&stream(&format!("\"{EXAMPLE_DPB}"))));
    assert!(!protected(&stream(&format!("{EXAMPLE_DPB}\""))));
    assert!(!protected(&stream("\"")));
    assert!(!protected(&stream("")));
    assert!(!protected("GC"));
}

#[test]
fn reads_the_key_whatever_its_case() {
    for key in ["GC", "gc", "Gc"] {
        assert!(protected(&format!("{key}=\"{EXAMPLE_DPB}\"")), "{key}");
    }
    // A longer key, or a shorter one, is another property.
    for key in ["GCx", "xGC", "G", " GC", "GC "] {
        assert!(!protected(&format!("{key}=\"{EXAMPLE_DPB}\"")), "{key}");
    }
}

#[test]
fn reads_the_first_gc_only() {
    let protecting = format!("{}\r\n", gc_line(EXAMPLE_DPB));
    let unprotecting = format!("{}\r\n", gc_line(EXAMPLE_GC));
    assert!(protected(&format!("{protecting}{unprotecting}")));
    assert!(!protected(&format!("{unprotecting}{protecting}")));
    // A GC that cannot be read is still the first one.
    assert!(!protected(&format!("GC=\"zz\"\r\n{protecting}")));
}

#[test]
fn reads_no_gc_after_the_first_section() {
    let gc = gc_line(EXAMPLE_DPB);
    for header in ["[Host Extender Info]", "[Workspace]", "[x"] {
        let stream = format!("Name=\"x\"\r\n{header}\r\n{gc}\r\n");
        assert!(!protected(&stream), "{header}");
    }
    // A bracket that does not open a line is part of a property.
    assert!(protected(&format!("Name=\"[x\"\r\n{gc}\r\n")));
}

#[test]
fn reads_every_line_ending() {
    let gc = gc_line(EXAMPLE_DPB);
    for ending in ["\r\n", "\n\r", "\n", "\r", "\r\n\r\n"] {
        let stream = format!("Name=\"x\"{ending}{gc}{ending}Id=\"y\"");
        assert!(protected(&stream), "{ending:?}");
    }
    assert!(protected(&gc));
}

// Code pages.

#[test]
fn reads_text_in_any_code_page() {
    let stream = with_gc(EXAMPLE_DPB);
    for code_page in [
        None,
        Some(1252),
        Some(1251),
        Some(932),
        Some(65001),
        Some(0),
    ] {
        assert!(
            parse_project(stream.as_bytes(), code_page).protected,
            "{code_page:?}"
        );
    }
    // Code page 1200 is UTF-16: the stream is read as such.
    let utf16: Vec<u8> = stream.encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert!(parse_project(&utf16, Some(1200)).protected);
    assert!(!parse_project(&utf16, Some(1252)).protected);
}

#[test]
fn reads_past_bytes_that_no_code_page_maps() {
    // The replacement character stands for them.
    let mut stream = vec![0x81, 0xFF, 0x80, b'\r', b'\n'];
    stream.extend_from_slice(gc_line(EXAMPLE_DPB).as_bytes());
    assert!(parse_project(&stream, Some(932)).protected);
}

// The type of a module.

/// A module of the `dir` stream, of which only the name and whether it is
/// procedural matter.
fn dir_module(name: &str, procedural: bool) -> DirModule {
    DirModule {
        name: name.to_string(),
        stream_name: name.to_string(),
        text_offset: 0,
        procedural,
    }
}

/// The properties that declare a module named `name`, with the type that each
/// gives it.
fn declarations(name: &str) -> [(String, ModuleType); 4] {
    [
        (format!("Document={name}/&H00000000"), ModuleType::Document),
        (format!("Class={name}"), ModuleType::Class),
        (format!("BaseClass={name}"), ModuleType::Form),
        (format!("Module={name}"), ModuleType::Standard),
    ]
}

/// The type that a stream, read in `code_page`, gives to a module that is not
/// procedural.
fn type_in_code_page(stream: &[u8], code_page: Option<u16>, name: &str) -> ModuleType {
    parse_project(stream, code_page).module_type(&dir_module(name, false))
}

/// The type that a stream, read in code page 1252, gives to a module that is
/// not procedural.
fn type_of(stream: &str, name: &str) -> ModuleType {
    type_in_code_page(stream.as_bytes(), Some(1252), name)
}

#[test]
fn reads_each_form_of_module_property() {
    for name in ["Foo", "My Module", " Padded ", "a=b", "a/b", "a/&Hb"] {
        for (line, expected) in declarations(name) {
            assert_eq!(type_of(&format!("{line}\r\n"), name), expected, "{line}");
            // The first declaration of a name wins. A second one, of any type,
            // shows that this one was read, even where it gives Document, the
            // type that a module gets when nothing declares it.
            for (other, _) in declarations(name) {
                let stream = format!("{line}\r\n{other}\r\n");
                assert_eq!(type_of(&stream, name), expected, "{line} then {other}");
            }
        }
    }
}

#[test]
fn reads_a_document_whatever_follows_its_name() {
    // The version of the Automation server is not checked, and may be missing.
    for line in [
        "Document=Foo/&H00000000",
        "Document=Foo/&hFFFFFFFF",
        "Document=Foo/&H",
        "Document=Foo/&H1",
        "Document=Foo",
    ] {
        // A class that comes second shows that this line was read: a module
        // that nothing declares is a document too.
        let stream = format!("{line}\r\nClass=Foo\r\n");
        assert_eq!(type_of(&stream, "Foo"), ModuleType::Document, "{line}");
    }
}

#[test]
fn skips_a_declaration_with_no_name() {
    // A module that nothing declares is a document, so only the other types
    // show that no module gets the empty name.
    for line in ["Class=", "BaseClass=", "Module="] {
        assert_eq!(type_of(line, ""), ModuleType::Document, "{line}");
    }
    let stream = "Document=\r\nDocument=/&H00000000\r\nClass=\r\nBaseClass=\r\nModule=\r\n";
    assert_eq!(type_of(stream, ""), ModuleType::Document);
}

#[test]
fn takes_a_module_that_no_property_names_for_a_document() {
    assert_eq!(type_of("", "Foo"), ModuleType::Document);
    // The properties name other modules, or none.
    let stream = concat!(
        "Class=Bar\r\n",
        "Package={AC9F2F90-E877-11CE-9F68-00AA00574A4F}\r\n",
        "Name=\"Foo\"\r\n",
    );
    assert_eq!(type_of(stream, "Foo"), ModuleType::Document);
    // The stream was read.
    assert_eq!(type_of(stream, "Bar"), ModuleType::Class);
}

#[test]
fn takes_a_procedural_module_for_a_standard_one() {
    let procedural = dir_module("Foo", true);
    let none = parse_project(b"", Some(1252));
    assert_eq!(none.module_type(&procedural), ModuleType::Standard);
    // Whatever the stream says: the dir stream is what knows.
    for (line, _) in declarations("Foo") {
        let info = parse_project(line.as_bytes(), Some(1252));
        assert_eq!(
            info.module_type(&procedural),
            ModuleType::Standard,
            "{line}"
        );
    }
}

#[test]
fn reads_the_key_of_a_module_property_whatever_its_case() {
    for (line, expected) in declarations("Foo") {
        for cased in [line.to_ascii_lowercase(), line.to_ascii_uppercase()] {
            for (other, other_type) in declarations("Foo") {
                if other_type != expected {
                    let stream = format!("{cased}\r\n{other}\r\n");
                    assert_eq!(type_of(&stream, "Foo"), expected, "{cased} then {other}");
                }
            }
        }
    }
}

#[test]
fn skips_a_property_whose_key_is_not_one_of_the_four() {
    // A longer key, a shorter one or a padded one is another property. Each
    // would give its module a type that comes first, and the class that comes
    // second shows that none does.
    for key in [
        " Document",
        "Document ",
        "Documents",
        "Documen",
        "BaseClasses",
        "BaseClas",
        " BaseClass",
        "Base Class",
        "Modules",
        "Modul",
        "xModule",
        " Module",
    ] {
        let stream = format!("{key}=Foo\r\nClass=Foo\r\n");
        assert_eq!(type_of(&stream, "Foo"), ModuleType::Class, "{key:?}");
    }
}

#[test]
fn reads_the_name_of_a_module_whatever_its_case() {
    // Classes show it: a module that nothing declares is a document.
    assert_eq!(
        type_of("Class=ThisWorkbook", "THISWORKBOOK"),
        ModuleType::Class
    );
    assert_eq!(
        type_of("Class=THISWORKBOOK", "thisworkbook"),
        ModuleType::Class
    );
    // More than ASCII: 0xC9 and 0xE9 are the capital and the small E acute of
    // code page 1252.
    let stream = b"Class=\xC9COLE";
    let class_named = |name| type_in_code_page(stream, Some(1252), name);
    assert_eq!(class_named("\u{e9}cole"), ModuleType::Class);
    assert_eq!(class_named("\u{c9}COLE"), ModuleType::Class);
    assert_eq!(class_named("ecole"), ModuleType::Document);
}

#[test]
fn reads_the_name_of_a_module_in_the_code_page_of_the_project() {
    // The name of a module in Russian, then in Japanese.
    let russian = "\u{41b}\u{438}\u{441}\u{442}1";
    let in_1251 = b"Class=\xCB\xE8\xF1\xF2\x31";
    assert_eq!(
        type_in_code_page(in_1251, Some(1251), russian),
        ModuleType::Class
    );
    // Read in another code page, the same bytes are another name.
    assert_eq!(
        type_in_code_page(in_1251, Some(1252), russian),
        ModuleType::Document
    );
    let japanese = "\u{65e5}\u{672c}\u{8a9e}";
    let in_932 = b"Class=\x93\xFA\x96\x7B\x8C\xEA";
    assert_eq!(
        type_in_code_page(in_932, Some(932), japanese),
        ModuleType::Class
    );
    assert_eq!(
        type_in_code_page(in_932, Some(1252), japanese),
        ModuleType::Document
    );
    // Code page 1200 is UTF-16.
    let utf16: Vec<u8> = format!("Class={russian}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(
        type_in_code_page(&utf16, Some(1200), russian),
        ModuleType::Class
    );
}

#[test]
fn reads_no_module_after_the_first_section() {
    for header in ["[Host Extender Info]", "[Workspace]", "[x"] {
        let stream = format!("Name=\"x\"\r\n{header}\r\nClass=Foo\r\n");
        assert_eq!(type_of(&stream, "Foo"), ModuleType::Document, "{header}");
    }
    // The properties that come before the section are read.
    let stream = "Class=Foo\r\n[Workspace]\r\nClass=Bar\r\n";
    assert_eq!(type_of(stream, "Foo"), ModuleType::Class);
    assert_eq!(type_of(stream, "Bar"), ModuleType::Document);
}

#[test]
fn reads_the_modules_of_the_example_stream() {
    let info = parse_project(EXAMPLE.as_bytes(), Some(1252));
    let type_named = |name: &str| info.module_type(&dir_module(name, false));
    assert_eq!(type_named("ThisWorkbook"), ModuleType::Document);
    assert_eq!(type_named("Sheet1"), ModuleType::Document);
    // The form shows that the stream was read.
    assert_eq!(type_named("UserForm1"), ModuleType::Form);
    // The example declares no other module.
    assert_eq!(type_named("Module1"), ModuleType::Document);
}

#[test]
fn reads_module_properties_with_every_line_ending() {
    for ending in ["\r\n", "\n\r", "\n", "\r"] {
        let stream = format!("Name=\"x\"{ending}Class=Foo{ending}Module=Bar{ending}");
        assert_eq!(type_of(&stream, "Foo"), ModuleType::Class, "{ending:?}");
        assert_eq!(type_of(&stream, "Bar"), ModuleType::Standard, "{ending:?}");
    }
}

// Limits.

#[test]
fn reads_no_further_than_the_most_bytes() {
    let line = gc_line(EXAMPLE_DPB);
    // The line ends with the last byte that is read.
    let mut inside = vec![b'\n'; MAX_PROJECT_STREAM.saturating_sub(line.len())];
    inside.extend_from_slice(line.as_bytes());
    assert_eq!(inside.len(), MAX_PROJECT_STREAM);
    assert!(parse_project(&inside, Some(1252)).protected);
    // One byte further, it loses its closing double quote.
    let mut outside = vec![b'\n'; MAX_PROJECT_STREAM.saturating_sub(line.len()) + 1];
    outside.extend_from_slice(line.as_bytes());
    assert_eq!(outside.len(), MAX_PROJECT_STREAM + 1);
    assert!(!parse_project(&outside, Some(1252)).protected);
}

#[test]
fn keeps_the_modules_of_the_first_declarations_only() {
    let stream: String = (0..=MAX_MODULES)
        .map(|index| format!("Class=m{index}\r\n"))
        .collect();
    let info = parse_project(stream.as_bytes(), Some(1252));
    let type_of_index = |index: usize| info.module_type(&dir_module(&format!("m{index}"), false));
    assert_eq!(type_of_index(0), ModuleType::Class);
    assert_eq!(
        type_of_index(MAX_MODULES.saturating_sub(1)),
        ModuleType::Class
    );
    // The declaration after the limit is not kept: a module that nothing
    // declares is a document.
    assert_eq!(type_of_index(MAX_MODULES), ModuleType::Document);
    // A name that is declared again takes no room.
    let repeated = format!(
        "{}Module=b\r\n",
        "Class=a\r\n".repeat(MAX_MODULES.saturating_add(10))
    );
    assert_eq!(type_of(&repeated, "b"), ModuleType::Standard);
}

// Hostile input.

#[test]
fn never_panics_on_damaged_streams() {
    let code_pages = [None, Some(1252), Some(932), Some(1200), Some(65001)];
    let mut lcg = Lcg::new(0x32);
    for round in 0..2_000usize {
        let code_page = code_pages.get(round % code_pages.len()).copied().unwrap();
        let random = lcg.random_bytes();
        let _ = parse_project(&random, code_page);
        let damaged = lcg.damage(EXAMPLE.as_bytes(), (round % 8).saturating_add(1));
        let info = parse_project(&damaged, code_page);
        // The names of the modules are as hostile as the stream.
        let name = String::from_utf8_lossy(&random).into_owned();
        let _ = info.module_type(&dir_module(&name, false));
        let _ = info.module_type(&dir_module("ThisWorkbook", false));
    }
    for length in 0..EXAMPLE.len() {
        let _ = parse_project(EXAMPLE.as_bytes().get(..length).unwrap(), Some(1252));
    }
}
