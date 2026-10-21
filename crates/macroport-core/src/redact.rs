// SPDX-License-Identifier: Apache-2.0
//! Redaction of a report for sharing outside the organisation (SPEC section 8,
//! ADR-0007).
//!
//! [`Redaction::apply`] replaces what names the analysed files with a keyed
//! hash and removes what quotes them. [`remove_snippets`] only removes the
//! quotes. Both rewrite the fleet report before any renderer reads it, so the
//! JSON, HTML and CSV reports carry the same values.
//!
//! The core draws no key: the caller injects it, and one key always gives one
//! report.

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::report::{FamilyScore, FileReport, ModuleReport, Report, Signal, VbaProject};

/// The text that takes the place of the message of every error.
const MESSAGE: &str = "[redacted]";

/// The bytes of a hash that are written: 8, which make 16 hexadecimal digits.
const HASH_BYTES: usize = 8;

/// The extensions that stay in a redacted path: those of the files that a scan
/// considers (SPEC M4), whatever their case. Any other text after the last dot
/// could come from a directory name, which is what redaction hides.
const KEPT_EXTENSIONS: [&str; 12] = [
    "xlsm", "xltm", "xlam", "xlsb", "xls", "xlt", "xla", "xlsx", "xltx", "bas", "cls", "frm",
];

/// The key cannot be used by HMAC-SHA256.
///
/// HMAC takes a key of any length, so no key raises this error. The `hmac`
/// crate types the case, and [`Redaction::new`] hands the type on instead of
/// panicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the key cannot be used by HMAC-SHA256")]
pub struct KeyError;

/// A redaction with its key: `--redact` of the command line.
///
/// A hash is the first 16 hexadecimal digits of the HMAC-SHA256 of the text
/// under the key. The same text gives the same hash wherever it appears, which
/// is what keeps the links of a report: the module named in a signal is the
/// module of the list.
#[derive(Clone)]
pub struct Redaction {
    /// The HMAC with the key already absorbed, cloned for each text.
    mac: Hmac<Sha256>,
    identifiers: bool,
}

// The state of the HMAC holds what the key gives, so it is not printed.
impl fmt::Debug for Redaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redaction")
            .field("identifiers", &self.identifiers)
            .finish_non_exhaustive()
    }
}

impl Redaction {
    /// A redaction under `key`, whose bytes may be of any number. With
    /// `identifiers`, the names that the project defines are hashed too.
    ///
    /// # Errors
    ///
    /// [`KeyError`], which HMAC never raises for a key of any length.
    pub fn new(key: &[u8], identifiers: bool) -> Result<Self, KeyError> {
        let mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| KeyError)?;
        Ok(Self { mac, identifiers })
    }

    /// Redacts `report` as the table of SPEC section 8 says:
    ///
    /// - the paths of the files and the roots of the scan are hashed, and keep
    ///   their extension when it is one that a scan covers;
    /// - the snippets are removed, and the message of every error is
    ///   `[redacted]` (its code stays);
    /// - the name of the VBA project is hashed;
    /// - with identifiers, so are the names of the modules and procedures, and
    ///   every other name of the project that the report holds: the stream of
    ///   a module, the procedures that a procedure calls, the module and the
    ///   procedure of a signal. A name is folded to lowercase first, since VBA
    ///   ignores case; in the calls, a name with dots is hashed part by part,
    ///   so that `Module1.Run` stays linked to the module and the procedure.
    ///
    /// Everything else stays: the references, the names of the APIs and the
    /// hashes of the contents, the metrics, families, signals and statuses.
    /// The files are sorted again, since the order of the paths would give
    /// away the order of the clear ones.
    pub fn apply(&self, report: &mut Report) {
        report.redacted = true;
        for root in report.scan.roots.iter_mut().flatten() {
            *root = self.path(root);
        }
        for file in &mut report.files {
            self.redact_file(file);
        }
        Report::sort_files(&mut report.files);
    }

    fn redact_file(&self, file: &mut FileReport) {
        file.path = self.path(&file.path);
        for error in file.errors.iter_mut().flatten() {
            error.message = MESSAGE.to_owned();
        }
        remove_snippets_of(file);
        if let Some(project) = file.vba_project.as_mut() {
            self.redact_project(project);
        }
        if self.identifiers {
            for signal in file.signals.iter_mut().flatten() {
                self.redact_signal(signal);
            }
        }
    }

    fn redact_project(&self, project: &mut VbaProject) {
        if let Some(name) = project.name.as_mut() {
            *name = self.hash(name);
        }
        if self.identifiers {
            for module in &mut project.modules {
                self.redact_module(module);
            }
        }
    }

    fn redact_module(&self, module: &mut ModuleReport) {
        module.name = self.identifier(&module.name);
        if let Some(stream_name) = module.stream_name.as_mut() {
            *stream_name = self.identifier(stream_name);
        }
        for signal in module.signals.iter_mut().flatten() {
            self.redact_signal(signal);
        }
        for procedure in &mut module.procedures {
            procedure.name = self.identifier(&procedure.name);
            for call in procedure.calls.iter_mut().flatten() {
                *call = self.qualified(call);
            }
            for signal in &mut procedure.signals {
                self.redact_signal(signal);
            }
        }
    }

    fn redact_signal(&self, signal: &mut Signal) {
        if let Some(module) = signal.module.as_mut() {
            *module = self.identifier(module);
        }
        if let Some(procedure) = signal.procedure.as_mut() {
            *procedure = self.identifier(procedure);
        }
    }

    /// `path` as a redacted report gives it: the hash of the whole path, then
    /// the extension if it stays.
    fn path(&self, path: &str) -> String {
        let mut redacted = self.hash(path);
        if let Some(extension) = kept_extension(path) {
            redacted.push('.');
            redacted.push_str(extension);
        }
        redacted
    }

    /// The hash of a name of the project, with the case of VBA: none.
    fn identifier(&self, name: &str) -> String {
        self.hash(&name.to_lowercase())
    }

    /// The hash of a name that may be qualified, each part by itself.
    fn qualified(&self, name: &str) -> String {
        let parts: Vec<String> = name.split('.').map(|part| self.identifier(part)).collect();
        parts.join(".")
    }

    /// The first 16 hexadecimal digits of the HMAC-SHA256 of `text`.
    fn hash(&self, text: &str) -> String {
        let mut mac = self.mac.clone();
        mac.update(text.as_bytes());
        let digit = |nibble: u8| char::from_digit(u32::from(nibble), 16).unwrap_or('0');
        mac.finalize()
            .into_bytes()
            .iter()
            .take(HASH_BYTES)
            .flat_map(|byte| [digit(byte >> 4), digit(byte & 0x0F)])
            .collect()
    }
}

/// Removes every snippet of `report`: `--no-snippets`, and the part of the
/// redaction that does not need a key.
///
/// The paths and the names stay, and `redacted` stays as it is: it says that
/// the paths are hashed too, which this does not do.
pub fn remove_snippets(report: &mut Report) {
    for file in &mut report.files {
        remove_snippets_of(file);
    }
}

/// Removes the snippet of every piece of evidence of `file`: those of its
/// families and signals, and those of its modules and procedures.
fn remove_snippets_of(file: &mut FileReport) {
    clear_scores(file.families.iter_mut().flatten());
    clear_signals(file.signals.iter_mut().flatten());
    let modules = file
        .vba_project
        .iter_mut()
        .flat_map(|project| &mut project.modules);
    for module in modules {
        clear_signals(module.signals.iter_mut().flatten());
        for procedure in &mut module.procedures {
            clear_scores(&mut procedure.families);
            clear_signals(&mut procedure.signals);
        }
    }
}

fn clear_scores<'a>(scores: impl IntoIterator<Item = &'a mut FamilyScore>) {
    for score in scores {
        for evidence in score.evidence.iter_mut().flatten() {
            evidence.snippet = None;
        }
    }
}

fn clear_signals<'a>(signals: impl IntoIterator<Item = &'a mut Signal>) {
    for signal in signals {
        for evidence in signal.evidence.iter_mut().flatten() {
            evidence.snippet = None;
        }
    }
}

/// The extension of the last name of `path` when it stays in a redacted path.
fn kept_extension(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next()?;
    let (_, extension) = name.rsplit_once('.')?;
    KEPT_EXTENSIONS
        .iter()
        .any(|kept| kept.eq_ignore_ascii_case(extension))
        .then_some(extension)
}
