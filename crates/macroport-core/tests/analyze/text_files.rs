// SPDX-License-Identifier: Apache-2.0
//! `analyze()` on a text module, a `.bas`, `.cls` or `.frm` file (SPEC, "Text
//! entry point"): one module in a file of `container: "text_module"`.

use macroport_core::analyze::{Options, analyze, analyze_modules};
use macroport_core::report::{Container, FileReport, Status};
use macroport_core::rules::ModuleType;
use macroport_core::vba::{MAX_LINES, ModuleSource};

use crate::support::{HELLO, checked, codes, module, options, sha256};

/// The report of the file `path` whose content is `bytes`, checked against the
/// schema.
fn text_file(path: &str, bytes: &[u8]) -> FileReport {
    checked(analyze(bytes, path, &options()))
}

#[test]
fn a_text_file_is_a_file_of_one_module() {
    let file = text_file("macros/Module1.bas", HELLO.as_bytes());

    assert_eq!(file.path, "macros/Module1.bas");
    assert_eq!(file.size, HELLO.len() as u64);
    assert_eq!(file.sha256, sha256(HELLO.as_bytes()));
    assert_eq!(
        (file.container, file.status),
        (Container::TextModule, Status::Ok)
    );
    assert_eq!(file.errors, Some(Vec::new()));
    let project = file.vba_project.as_ref().expect("a VBA project");
    assert_eq!((project.protected, project.code_page), (false, None));
    let module = module(&file);
    assert_eq!(module.name, "Module1");
    assert_eq!(module.module_type, ModuleType::Standard);
    assert_eq!(module.stream_name, None);
    assert!(module.source_available);
    assert_eq!(module.source_sha256, Some(sha256(HELLO.as_bytes())));
    assert_eq!(module.procedures.len(), 1);
}

#[test]
fn a_text_file_is_typed_by_its_extension_and_named_after_its_stem() {
    for (path, name, expected) in [
        ("Module1.bas", "Module1", ModuleType::Standard),
        ("some/dir/Tools.BAS", "Tools", ModuleType::Standard),
        ("C:\\macros\\UserForm1.frm", "UserForm1", ModuleType::Form),
        ("Helper.cls", "Helper", ModuleType::Class),
        ("my.module.bas", "my.module", ModuleType::Standard),
        (".bas", "Module", ModuleType::Standard),
    ] {
        let file = text_file(path, HELLO.as_bytes());

        assert_eq!(file.container, Container::TextModule, "{path}");
        let module = module(&file);
        assert_eq!(
            (module.name.as_str(), module.module_type),
            (name, expected),
            "{path}"
        );
    }
}

#[test]
fn a_cls_file_is_a_document_with_workbook_or_worksheet_procedures_or_vb_base() {
    let base = "Attribute VB_Base = \"0{00020820-0000-0000-C000-000000000046}\"\n";
    for (text, expected) in [
        (
            "Private Sub Workbook_Open()\nEnd Sub\n",
            ModuleType::Document,
        ),
        (
            "Private Sub WORKSHEET_Change(ByVal T As Range)\nEnd Sub\n",
            ModuleType::Document,
        ),
        (
            "Sub Other()\nEnd Sub\nSub workbook_BeforeClose()\nEnd Sub\n",
            ModuleType::Document,
        ),
        (base, ModuleType::Document),
        ("Sub Workbook()\nEnd Sub\n", ModuleType::Class),
        ("' Workbook_Open\nSub A()\nEnd Sub\n", ModuleType::Class),
        ("Sub Open_Workbook_Now()\nEnd Sub\n", ModuleType::Class),
        (
            "Attribute VB_Name = \"Foo\"\nSub A()\nEnd Sub\n",
            ModuleType::Class,
        ),
        ("", ModuleType::Class),
    ] {
        let file = text_file("Sheet1.cls", text.as_bytes());

        assert_eq!(module(&file).module_type, expected, "{text:?}");
    }
}

#[test]
fn the_extension_alone_types_a_bas_and_a_frm_file() {
    let text = b"Attribute VB_Base = \"0{x}\"\nPrivate Sub Workbook_Open()\nEnd Sub\n";

    let standard = text_file("a.bas", text);
    let form = text_file("a.frm", text);

    assert_eq!(module(&standard).module_type, ModuleType::Standard);
    assert_eq!(module(&form).module_type, ModuleType::Form);
    assert_eq!(form.metrics.expect("metrics").forms, Some(1));
}

#[test]
fn the_size_and_hash_of_a_text_file_are_those_of_its_bytes() {
    let bytes = b"Sub A()\r\n    x = 1\r\nEnd Sub\r\n";

    let file = text_file("A.bas", bytes);

    assert_eq!(file.size, bytes.len() as u64);
    assert_eq!(file.sha256, sha256(bytes));
    // The source of the module has the line endings of the decoded text.
    let module = module(&file);
    assert_eq!(
        module.source_sha256,
        Some(sha256(b"Sub A()\n    x = 1\nEnd Sub\n"))
    );
    assert_eq!((module.metrics.loc, module.procedures[0].end_line), (3, 3));
}

#[test]
fn a_text_file_is_read_as_utf8_utf16_or_windows_1252() {
    let decoded = "' caf\u{e9}\nSub A()\nEnd Sub\n";
    let utf8_with_mark = [&b"\xEF\xBB\xBF"[..], decoded.as_bytes()].concat();
    let ansi = b"' caf\xE9\nSub A()\nEnd Sub\n";
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(decoded.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();

    for (what, bytes) in [
        ("UTF-8", decoded.as_bytes()),
        ("UTF-8 with a mark", &utf8_with_mark[..]),
        ("windows-1252", &ansi[..]),
        ("UTF-16LE", &utf16[..]),
    ] {
        let file = text_file("A.bas", bytes);

        let module = module(&file);
        assert_eq!(
            module.source_sha256,
            Some(sha256(decoded.as_bytes())),
            "{what}"
        );
        let a = &module.procedures[0];
        assert_eq!(
            (a.name.as_str(), a.start_line, a.end_line),
            ("A", 2, 3),
            "{what}"
        );
        assert_eq!(file.sha256, sha256(bytes), "{what}");
    }
}

#[test]
fn the_lines_of_a_text_file_count_whatever_ends_them() {
    // Excel exports with CR LF, and the corpus samples have LF.
    for bytes in [
        &b"Sub A()\n\nEnd Sub\n"[..],
        b"Sub A()\r\n\r\nEnd Sub\r\n",
        b"Sub A()\r\rEnd Sub\r",
        b"Sub A()\n\nEnd Sub",
    ] {
        let file = text_file("A.bas", bytes);

        let a = &module(&file).procedures[0];
        assert_eq!(
            (a.start_line, a.end_line, a.metrics.loc),
            (1, 3, 3),
            "{bytes:?}"
        );
    }
}

#[test]
fn a_file_without_a_procedure_has_no_macros_and_is_still_described() {
    for bytes in [
        &b""[..],
        b"Option Explicit\nDim x As Long\n",
        b"\xEF\xBB\xBF",
    ] {
        let file = text_file("A.bas", bytes);

        assert_eq!(file.status, Status::NoMacros, "{bytes:?}");
        assert_eq!(file.container, Container::TextModule);
        assert!(module(&file).procedures.is_empty(), "{bytes:?}");
    }
}

#[test]
fn analyze_gives_what_analyze_modules_gives_for_the_same_text() {
    let by_modules = analyze_modules(
        vec![ModuleSource {
            name: "Module1".to_owned(),
            module_type: ModuleType::Standard,
            text: HELLO.to_owned(),
        }],
        "dir/Module1.bas",
        &options(),
    );

    assert_eq!(text_file("dir/Module1.bas", HELLO.as_bytes()), by_modules);
}

#[test]
fn a_zip_archive_or_a_compound_file_is_no_text_whatever_its_name() {
    for (path, bytes) in [
        ("Module1.bas", &b"PK\x03\x04 not a package"[..]),
        (
            "Sheet1.cls",
            b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1 not a workbook",
        ),
    ] {
        let file = analyze(bytes, path, &options());

        assert_ne!(file.container, Container::TextModule, "{path}");
        assert_ne!(file.status, Status::Ok, "{path}");
    }
}

#[test]
fn any_other_bytes_are_read_as_text_without_failing() {
    let bytes: Vec<u8> = (0..=u8::MAX).cycle().take(10_000).collect();

    let file = text_file("Noise.bas", &bytes);

    assert_eq!(file.container, Container::TextModule);
    assert_eq!(file.sha256, sha256(&bytes));
    assert!(matches!(file.status, Status::Ok | Status::NoMacros));
}

#[test]
fn a_text_file_above_the_limit_is_too_large_and_not_read() {
    let limited = Options {
        max_file_size: 10,
        ..options()
    };

    let file = checked(analyze(HELLO.as_bytes(), "Module1.bas", &limited));

    assert_eq!(
        (file.container, file.status),
        (Container::TextModule, Status::TooLarge)
    );
    assert_eq!(file.size, HELLO.len() as u64);
    assert_eq!(file.sha256, sha256(HELLO.as_bytes()));
    assert!(file.vba_project.is_none());
    assert_eq!(codes(&file), ["file.too_large"]);
}

#[test]
fn a_text_file_above_eight_mebibytes_is_too_large_whatever_the_option_allows() {
    let cap = 8 * 1024 * 1024;
    let lenient = Options {
        max_file_size: u64::MAX,
        ..options()
    };

    let at_cap = checked(analyze(&vec![b'a'; cap], "Big.bas", &lenient));
    let over = checked(analyze(&vec![b'a'; cap + 1], "Big.bas", &lenient));

    assert_eq!(at_cap.status, Status::NoMacros);
    assert_eq!(over.status, Status::TooLarge);
    assert_eq!(over.container, Container::TextModule);
    assert_eq!(over.size, cap as u64 + 1);
    assert!(over.vba_project.is_none());
    assert_eq!(codes(&over), ["file.too_large"]);
    assert!(over.errors.expect("errors")[0].message.contains("8388608"));
}

#[test]
fn a_text_file_of_too_many_lines_is_too_large_and_its_module_is_listed_unread() {
    let big = "a\n".repeat(MAX_LINES as usize + 1);

    let file = text_file("Big.bas", big.as_bytes());

    assert_eq!(file.status, Status::TooLarge);
    assert_eq!(codes(&file), ["vba.too_many_lines"]);
    let message = &file.errors.as_ref().expect("errors")[0].message;
    assert_eq!(message, &format!("module Big: more than {MAX_LINES} lines"));
    let module = module(&file);
    assert!(module.source_available);
    assert_eq!((module.procedures.len(), module.metrics.loc), (0, 0));
}
