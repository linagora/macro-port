// SPDX-License-Identifier: Apache-2.0
//! The `PROJECT` stream of a VBA project (MS-OVBA section 2.3.1): text in the
//! code page of the project, with one `Key=value` property per line. Its
//! properties come first, and the sections that follow (`[Host Extender
//! Info]`, `[Workspace]`) are of no use here.

use std::collections::BTreeMap;

use super::encryption::decrypt;
use super::text::decode_mbcs;
use super::{DirModule, MAX_MODULES, MAX_PROJECT_STREAM};
use crate::rules::ModuleType;

/// The visibility state of a project that is not visible: a protected one.
const NOT_VISIBLE: u8 = 0x00;

/// The keys of the properties that declare a module, with the type that each
/// gives it. `BaseClass` declares a designer, which is a form here.
const DECLARATIONS: [(&str, ModuleType); 4] = [
    ("Document", ModuleType::Document),
    ("Class", ModuleType::Class),
    ("BaseClass", ModuleType::Form),
    ("Module", ModuleType::Standard),
];

/// What the `PROJECT` stream says about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    /// Whether the project is protected: its visibility state (the `GC`
    /// property), once decrypted, is 0x00, which is not visible. A project
    /// whose `GC` is missing or cannot be read is not protected. No password
    /// is recovered, and the source of the modules is not encrypted either
    /// way.
    pub protected: bool,
    /// The modules that the properties declare, by their name once folded.
    declared: BTreeMap<String, ModuleType>,
}

impl ProjectInfo {
    /// The type of a module that the `dir` stream lists.
    ///
    /// A procedural module is a standard module, whatever the stream says.
    /// Any other module has the type that a `Document`, `Class`, `BaseClass`
    /// (a form) or `Module` (standard) property gives to its name, whatever
    /// the case of the name. A module that no property names is taken for a
    /// document, the type that the rules of auto-execution and of event
    /// handlers look at: a stream cannot escape them by leaving a line out.
    #[must_use]
    pub fn module_type(&self, module: &DirModule) -> ModuleType {
        if module.procedural {
            return ModuleType::Standard;
        }
        self.declared
            .get(&fold(&module.name))
            .copied()
            .unwrap_or(ModuleType::Document)
    }
}

/// A module name as `ProjectInfo` keys it: the case of a name does not tell
/// two modules apart.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Reads a `PROJECT` stream.
///
/// `code_page` is the code page of the project, which its `dir` stream gives
/// (see [`parse_dir`](super::parse_dir)). The stream is hostile like any
/// other, and a line that is not understood is skipped, so reading does not
/// fail. The key of a property is read whatever its case, the first property
/// of a kind wins (the first `GC`, the first declaration of a module name), the
/// properties end at the first line that opens a section (`[`), no more than
/// the first [`MAX_PROJECT_STREAM`] bytes of the stream are read (a longer
/// stream is cut there, not rejected), and no more than [`MAX_MODULES`] module
/// names are kept.
#[must_use]
pub fn parse_project(stream: &[u8], code_page: Option<u16>) -> ProjectInfo {
    let stream = stream.get(..MAX_PROJECT_STREAM).unwrap_or(stream);
    let text = decode_mbcs(stream, code_page);
    let mut protected = None;
    let mut declared = BTreeMap::new();
    // MS-OVBA ends a line with CR LF or LF CR. Splitting at every CR and LF
    // reads both, and either alone; the empty lines that result are no
    // property.
    for line in text.split(['\r', '\n']) {
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.eq_ignore_ascii_case("GC") {
            protected.get_or_insert_with(|| says_protected(value));
        } else if let Some((name, module_type)) = declaration(key, value)
            && declared.len() < MAX_MODULES
        {
            declared.entry(fold(name)).or_insert(module_type);
        }
    }
    ProjectInfo {
        protected: protected.unwrap_or(false),
        declared,
    }
}

/// The module that a property declares, if it is a `Document`, `Class`,
/// `BaseClass` or `Module` property: its name, and its type. A name is not
/// empty.
fn declaration<'a>(key: &str, value: &'a str) -> Option<(&'a str, ModuleType)> {
    let &(_, module_type) = DECLARATIONS
        .iter()
        .find(|(property, _)| key.eq_ignore_ascii_case(property))?;
    let name = if module_type == ModuleType::Document {
        // The name is followed by the version of the Automation server of the
        // document: `/&H` and 8 hexadecimal digits. Like a key, `&H` is read
        // in either case (ABNF strings are not case sensitive).
        value
            .to_ascii_lowercase()
            .rfind("/&h")
            .and_then(|end| value.get(..end))
            .unwrap_or(value)
    } else {
        value
    };
    (!name.is_empty()).then_some((name, module_type))
}

/// Whether the value of a `GC` property, the hexadecimal digits of an Encrypted
/// Data Structure between double quotes, holds one byte that is 0x00: it says
/// that the project is protected.
fn says_protected(value: &str) -> bool {
    let digits = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'));
    let Some(structure) = digits.and_then(hex_bytes) else {
        return false;
    };
    decrypt(&structure).is_some_and(|data| data == [NOT_VISIBLE])
}

/// The bytes that hexadecimal digits write, in either case.
fn hex_bytes(digits: &str) -> Option<Vec<u8>> {
    let (pairs, odd) = digits.as_bytes().as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[high, low]| Some((nibble(high)? << 4) | nibble(low)?))
        .collect()
}

/// The value of a hexadecimal digit.
fn nibble(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}
