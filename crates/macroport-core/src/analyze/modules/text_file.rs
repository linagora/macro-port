// SPDX-License-Identifier: Apache-2.0
//! A text module, a `.bas`, `.cls` or `.frm` file, as a file of its own (SPEC,
//! "Text entry point"): `container: "text_module"`, with one module.
//!
//! The module is named after the file. Its type comes from the extension, and a
//! `.cls` file is a document module when it has `Workbook_*` or `Worksheet_*`
//! procedures or an `Attribute VB_Base` line, a class module otherwise. The
//! `size` and the `sha256` of the file are those of its bytes, and the source of
//! the module is what [`decode_file`] makes of them.

use super::text::{capped, text_extraction, text_module, too_large_text};
use super::{Analysis, Front};
use crate::analyze::{Options, detect, sha256_hex};
use crate::ovba::decode_text;
use crate::report::FileReport;
use crate::rules::ModuleType;

/// The most bytes that a text file may have: as many as a module of a VBA
/// project may have (`MAX_MODULE_SOURCE` in `project.rs`). Decoded, a text that
/// is not UTF-8 takes up to three times its size, and the lines that the front
/// end makes of it as much again, so the 32 MiB that texts may total in
/// `analyze_modules` would go past the 256 MB per file of SPEC M1. The limit of
/// [`Options::max_file_size`] applies as well when it is lower.
const MAX_TEXT_FILE_SIZE: u64 = 8 * 1024 * 1024;

/// The byte order mark of UTF-16 little-endian text.
const UTF16_LE_MARK: [u8; 2] = [0xFF, 0xFE];

/// The byte order mark of UTF-8 text.
const UTF8_MARK: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// The code page of UTF-16 little-endian text, which `decode_text` knows.
const UTF16_LE: u16 = 1200;

/// The code page of UTF-8 text.
const UTF8: u16 = 65001;

/// The code page that reads any byte, for a text that is not UTF-8.
const WINDOWS_1252: u16 = 1252;

/// What the names of the procedures of a document module start with, in
/// lowercase: the events of the workbook and of the worksheets.
const DOCUMENT_EVENTS: [&str; 2] = ["workbook_", "worksheet_"];

/// Whether `bytes`, a file named `path`, are a text module: the extension says
/// so, and the bytes are not those of a ZIP archive or a compound file, which
/// are workbooks or packages whatever their name.
pub(in crate::analyze) fn is_text_module(bytes: &[u8], path: &str) -> bool {
    detect::is_text_extension(&detect::extension(path))
        && !detect::is_zip(bytes)
        && !detect::is_compound(bytes)
}

/// Analyses a text module, a file that [`is_text_module`] accepts. A text above
/// the size limit is not read, and neither is one of too many lines: its module
/// is listed without a measure, and the file is `too_large`.
pub(in crate::analyze) fn analyze_text(
    bytes: &[u8],
    path: &str,
    options: &Options<'_>,
) -> FileReport {
    let options = capped(options, MAX_TEXT_FILE_SIZE);
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let sha256 = sha256_hex(bytes);
    if size > options.max_file_size {
        return too_large_text(path, size, sha256, &options);
    }
    let text = decode_file(bytes);
    let name = module_name(path);
    let mut front = Front::new(options.rules);
    let analysis = front.analyse(&text);
    let module_type = module_type(&detect::extension(path), analysis.as_ref().ok());
    let mut module = text_module(&name, module_type, &text);
    match analysis {
        Ok(analysis) => front.fill(&mut module, &analysis),
        Err(error) => front.fail(&name, error),
    }
    let extraction = text_extraction(path, size, sha256, vec![module], Vec::new(), options.rules);
    let mut file = extraction.file;
    front.seal(&mut file);
    file
}

/// The text of a file of VBA source. VBA exports it in the code page of the
/// machine, so a text that is not UTF-8 is read as windows-1252, which gives
/// every byte a character. A UTF-16 little-endian text is recognised by its
/// byte order mark, and UTF-16 big-endian is not read as such. A byte that the
/// encoding does not map becomes U+FFFD, a byte order mark is not part of the
/// text, and the line terminators become `\n`.
fn decode_file(bytes: &[u8]) -> String {
    let (rest, code_page) = if let Some(rest) = bytes.strip_prefix(&UTF16_LE_MARK) {
        (rest, UTF16_LE)
    } else if let Some(rest) = bytes.strip_prefix(&UTF8_MARK) {
        (rest, UTF8)
    } else if std::str::from_utf8(bytes).is_ok() {
        (bytes, UTF8)
    } else {
        (bytes, WINDOWS_1252)
    };
    decode_text(rest, Some(code_page))
}

/// The name of the module that a file holds: the name of the file without its
/// extension, or `Module` when nothing is left of it.
fn module_name(path: &str) -> String {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, _) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
    if stem.is_empty() { "Module" } else { stem }.to_owned()
}

/// The type of the module of a file with this `extension`: a form for `.frm`, a
/// standard module for `.bas`, and for `.cls` a document when `analysis`, which
/// is missing for a module too long to be read, shows it is one.
fn module_type(extension: &str, analysis: Option<&Analysis>) -> ModuleType {
    match extension {
        "bas" => ModuleType::Standard,
        "frm" => ModuleType::Form,
        _ if analysis.is_some_and(is_document) => ModuleType::Document,
        _ => ModuleType::Class,
    }
}

/// Whether a class file is the module of a document: it has procedures named
/// `Workbook_*` or `Worksheet_*`, or the attribute `VB_Base`, which Excel writes
/// for the classes behind the workbook and the sheets.
fn is_document(analysis: &Analysis) -> bool {
    let event = |name: &str| {
        DOCUMENT_EVENTS.iter().any(|prefix| {
            let head = name.get(..prefix.len());
            head.is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        })
    };
    analysis.lines.attribute("VB_Base").is_some()
        || (analysis.module.procedures.iter()).any(|procedure| event(&procedure.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_module_is_named_after_its_file() {
        for (path, name) in [
            ("Module1.bas", "Module1"),
            ("dir/Sub.Dir/Tools.CLS", "Tools"),
            ("C:\\a\\b\\Form1.frm", "Form1"),
            ("my.module.bas", "my.module"),
            ("noextension", "noextension"),
            (".bas", "Module"),
            ("dir/", "Module"),
            ("", "Module"),
        ] {
            assert_eq!(module_name(path), name, "{path}");
        }
    }

    #[test]
    fn only_a_file_that_is_no_package_with_a_module_extension_is_a_text_module() {
        for (path, bytes) in [
            ("a.bas", &b"Sub A()"[..]),
            ("a.CLS", b""),
            ("dir/a.frm", b"VERSION 5.00"),
        ] {
            assert!(is_text_module(bytes, path), "{path}");
        }
        for (path, bytes) in [
            ("a.xlsm", &b"Sub A()"[..]),
            ("a.txt", b"Sub A()"),
            ("a", b"Sub A()"),
            ("a.bas", b"PK\x03\x04"),
            ("a.cls", b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1"),
        ] {
            assert!(!is_text_module(bytes, path), "{path}");
        }
    }

    #[test]
    fn a_text_is_decoded_into_the_text_it_presents() {
        for (bytes, text) in [
            (&b"abc"[..], "abc"),
            (b"", ""),
            (b"caf\xC3\xA9", "caf\u{e9}"),
            (b"\xEF\xBB\xBFcaf\xC3\xA9", "caf\u{e9}"),
            (b"caf\xE9", "caf\u{e9}"),
            (b"\xFF\xFEc\0a\0f\0\xE9\0", "caf\u{e9}"),
            (b"a\r\nb\rc\nd", "a\nb\nc\nd"),
            (b"\xFF\xFEa\0\r\0\n\0b\0", "a\nb"),
            // An odd byte left over after UTF-16 text is a replacement character.
            (b"\xFF\xFEa\0b", "a\u{fffd}"),
            // Big-endian UTF-16 is read as windows-1252, which loses no byte.
            (b"\xFE\xFF\0a", "\u{fe}\u{ff}\0a"),
        ] {
            assert_eq!(decode_file(bytes), text, "{bytes:?}");
        }
    }
}
