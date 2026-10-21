// SPDX-License-Identifier: Apache-2.0
//! [`analyze`]: the bytes of one file in, its [`FileReport`] out.
//!
//! This is the extraction stage (SPEC M1): what kind of file it is, whether it
//! can be read, and the modules of its VBA project with their source. The
//! stages that read the source (M2, M3) fill the fields that extraction leaves
//! empty. [`extract`] runs the extraction alone and also hands back the source
//! of each module, which the report does not carry.
//!
//! A ZIP archive is read as an OOXML package, and a compound file as a BIFF8
//! workbook (`.xls`) unless it is an encrypted package, which is not opened.
//! The extension only picks the variant of the format within that family: a
//! file whose extension names another family has the container `unknown`. A
//! file is `ok` if it has Excel 4.0 macro sheets (XLM), if a module holds code
//! (a stopgap for the procedures that SPEC M1 asks for and M2 will find) or if
//! it is a workbook whose encryption hides its sheets; else it is `no_macros`,
//! even with a VBA project of empty modules. A file that could not be read has
//! the status of the first failure, and the report keeps what could be read: a
//! module that could not be read is listed without its source, and a workbook
//! that could not be read still has its project described.
//!
//! Nothing here executes or evaluates anything from the file, and nothing
//! panics on its content: every failure is a status and an error code in the
//! report. The codes that this module gives, besides those of the container
//! and MS-OVBA layers:
//!
//! | code | status | meaning |
//! | --- | --- | --- |
//! | `file.too_large` | `too_large` | the file is above [`Options::max_file_size`] |
//! | `file.unsupported_format` | `unsupported` | the format of the file is not read |
//! | `file.encrypted_package` | `encrypted` | an encrypted OOXML package, which is not decrypted |
//! | `biff.workbook.missing` | `corrupted` | a workbook with a VBA storage has no `Workbook` stream |
//! | `ovba.dir.missing` | `corrupted` | the project has no `dir` stream |
//! | `ovba.module.missing_stream` | `corrupted` | the stream of a module is not in the project |
//! | `ovba.project.too_large` | `too_large` | the `PROJECT` stream is above the limit of the MS-OVBA layer |
//!
//! The message of an error about a module starts with `module <name>:`.

mod detect;
mod project;

use sha2::{Digest, Sha256};

use self::project::Project;
use crate::container::{CfbFile, ContainerError, read_biff_workbook, read_ooxml};
use crate::ovba::OvbaError;
use crate::report::{Container, FileError, FileReport, Orientation, Status, Xlm};
use crate::rules::RuleSet;

/// The largest file that [`analyze`] reads by default: 200 MiB. SPEC M1 says
/// 200 MB, and like the other limits of the container layer, it takes a
/// megabyte for 1,048,576 bytes.
pub const DEFAULT_MAX_FILE_SIZE: u64 = 200 * 1024 * 1024;

/// The storage of `xl/vbaProject.bin` that holds the VBA project: its root.
const OOXML_PROJECT_STORAGE: &str = "";

/// The storage of an `.xls` workbook that holds its VBA project.
const BIFF8_PROJECT_STORAGE: &str = "_VBA_PROJECT_CUR";

/// The stream of an `.xls` workbook that holds its sheets and its encryption.
const BIFF8_WORKBOOK_STREAM: &str = "Workbook";

/// What the caller decides about an analysis.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    /// The rules. Nothing reads them yet: the container indicators that
    /// extraction will raise take their severity from them.
    pub rules: &'a RuleSet,
    /// The largest file, in bytes, that is read. A larger one gets the status
    /// `too_large` and is not opened: its report still gives its size and
    /// hash.
    pub max_file_size: u64,
}

impl<'a> Options<'a> {
    /// The default options for `rules`: files up to [`DEFAULT_MAX_FILE_SIZE`].
    pub fn new(rules: &'a RuleSet) -> Self {
        Self {
            rules,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

/// What [`extract`] found in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Extraction {
    /// The report of the file, as far as extraction fills it.
    pub file: FileReport,
    /// The source of each module of `file.vba_project`, in the same order: the
    /// text that the line numbers of the evidence count in. `None` for a
    /// module whose source could not be read. Empty when the file has no VBA
    /// project.
    pub sources: Vec<Option<String>>,
}

/// Analyses one file: `bytes` are its content, `path` its path as the report
/// gives it (its extension also picks the variant of the format).
///
/// The result never fails: a file that cannot be read gets a status other
/// than `ok` and `no_macros`, and the errors that explain it.
pub fn analyze(bytes: &[u8], path: &str, options: &Options<'_>) -> FileReport {
    extract(bytes, path, options).file
}

/// Runs the extraction stage alone.
///
/// A file above [`Options::max_file_size`] gets the status `too_large` and is
/// not opened. Otherwise its first bytes decide how it is read: a ZIP archive
/// is an OOXML package, a compound file a BIFF8 workbook or an encrypted
/// package, and anything else is `unsupported`.
pub fn extract(bytes: &[u8], path: &str, options: &Options<'_>) -> Extraction {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let reading = if size > options.max_file_size {
        too_large_reading(path, options)
    } else {
        read(bytes, &detect::extension(path))
    };
    reading.into_extraction(path, size, sha256_hex(bytes))
}

/// The report of a file above [`Options::max_file_size`], given its `size` and
/// the `sha256` of its content.
///
/// [`analyze`] gives it for the files it is handed. A caller that does not
/// want to hold such a file in memory can read its size from the file system,
/// hash it as it streams it, and ask for this report instead.
pub fn too_large(path: &str, size: u64, sha256: String, options: &Options<'_>) -> FileReport {
    let reading = too_large_reading(path, options);
    reading.into_extraction(path, size, sha256).file
}

fn too_large_reading(path: &str, options: &Options<'_>) -> Reading {
    let failure = Failure::new(
        Status::TooLarge,
        "file.too_large",
        format!(
            "the file is larger than the limit of {} bytes",
            options.max_file_size
        ),
    );
    let container = detect::container_by_extension(&detect::extension(path));
    Reading::failed(container, failure)
}

/// The SHA-256 of `bytes` as 64 lowercase hexadecimal digits.
fn sha256_hex(bytes: &[u8]) -> String {
    let digit = |nibble: u8| char::from_digit(u32::from(nibble), 16).unwrap_or('0');
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [digit(byte >> 4), digit(byte & 0x0F)])
        .collect()
}

/// Why a file, or a module of it, could not be read: the status it gives the
/// file, and the error that the report lists.
struct Failure {
    status: Status,
    error: FileError,
}

impl Failure {
    fn new(status: Status, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            error: FileError {
                code: code.to_owned(),
                message: message.into(),
            },
        }
    }

    /// The same failure, said to be that of the module `name`.
    fn in_module(mut self, name: &str) -> Self {
        self.error.message = format!("module {name}: {}", self.error.message);
        self
    }
}

impl From<ContainerError> for Failure {
    fn from(error: ContainerError) -> Self {
        Self::new(error.status, error.code, error.message)
    }
}

impl From<OvbaError> for Failure {
    fn from(error: OvbaError) -> Self {
        Self::new(error.status, error.code, error.message)
    }
}

/// What reading a file found: its container, its VBA project and its XLM
/// sheets if it has any, and what went wrong.
struct Reading {
    container: Container,
    project: Option<Project>,
    /// `None` when the sheets could not be counted.
    xlm: Option<Xlm>,
    /// `None` when the workbook was not read.
    workbook_encrypted: Option<bool>,
    /// In the order they were met: the status of the file is that of the
    /// first.
    failures: Vec<Failure>,
}

impl Reading {
    /// A file in which nothing has been found yet.
    fn new(container: Container) -> Self {
        Self {
            container,
            project: None,
            xlm: None,
            workbook_encrypted: None,
            failures: Vec::new(),
        }
    }

    fn failed(container: Container, failure: Failure) -> Self {
        Self {
            failures: vec![failure],
            ..Self::new(container)
        }
    }

    /// Reads the VBA project in the storage at `storage_path` of `compound`.
    fn read_project(&mut self, compound: &mut CfbFile<'_>, storage_path: &str) {
        match project::read(compound, storage_path) {
            Ok((project, failures)) => {
                self.project = Some(project);
                self.failures.extend(failures);
            }
            Err(failure) => self.failures.push(failure),
        }
    }

    /// Reads what the `Workbook` stream says: whether the workbook is
    /// encrypted, and how many macro sheets it has.
    fn read_workbook(&mut self, stream: &[u8]) {
        match read_biff_workbook(stream) {
            Ok(info) => {
                self.workbook_encrypted = Some(info.file_pass);
                self.xlm = info.macro_sheets.map(xlm_of);
            }
            Err(error) => self.failures.push(error.into()),
        }
    }

    /// The status of the file: that of the first failure if there is one.
    /// Otherwise `ok` when a module holds code (the stopgap for a procedure,
    /// which M2 will find), when there are macro sheets, or when the
    /// encryption of the workbook hides the sheets that might be, and
    /// `no_macros` when none of these holds.
    fn status(&self) -> Status {
        if let Some(failure) = self.failures.first() {
            return failure.status;
        }
        let code = self.project.as_ref().is_some_and(Project::has_code);
        let sheets = self.xlm.as_ref().is_some_and(|xlm| xlm.present);
        if code || sheets || self.workbook_encrypted == Some(true) {
            Status::Ok
        } else {
            Status::NoMacros
        }
    }

    /// The extraction of the file `path`, of `size` bytes and the hash
    /// `sha256`. Everything the stages after extraction fill is left empty.
    fn into_extraction(self, path: &str, size: u64, sha256: String) -> Extraction {
        let status = self.status();
        let (vba_project, sources) = match self.project {
            Some(project) => (Some(project.report), project.sources),
            None => (None, Vec::new()),
        };
        let file = FileReport {
            path: path.to_owned(),
            size,
            sha256,
            container: self.container,
            status,
            errors: Some(self.failures.into_iter().map(|f| f.error).collect()),
            vba_project,
            xlm: self.xlm,
            metrics: None,
            complexity_level: None,
            primary_family: None,
            families: None,
            max_severity: None,
            signals: None,
            traits: None,
            orientation: Orientation,
            workbook_encrypted: self.workbook_encrypted,
            notes: None,
        };
        Extraction { file, sources }
    }
}

/// The macro sheets of a file that has `count` of them.
fn xlm_of(count: u32) -> Xlm {
    Xlm {
        present: count > 0,
        macro_sheets: Some(count),
    }
}

fn unsupported_format(message: &str) -> Failure {
    Failure::new(Status::Unsupported, "file.unsupported_format", message)
}

/// Reads a file that is within the size limit.
fn read(bytes: &[u8], extension: &str) -> Reading {
    if detect::is_zip(bytes) {
        return read_package(bytes, extension);
    }
    if detect::is_compound(bytes) {
        return read_compound(bytes, extension);
    }
    // Nothing but the extension says what the file was meant to be.
    let container = detect::container_by_extension(extension);
    let failure = unsupported_format("the format of the file is not read");
    Reading::failed(container, failure)
}

/// An OOXML package: its macro sheets are counted, it is not encrypted (an
/// encrypted package is a compound file) and its VBA project is the compound
/// file `xl/vbaProject.bin`.
fn read_package(bytes: &[u8], extension: &str) -> Reading {
    let container = detect::ooxml_container(extension);
    let package = match read_ooxml(bytes) {
        Ok(package) => package,
        Err(error) => return Reading::failed(container, error.into()),
    };
    let mut reading = Reading::new(container);
    reading.workbook_encrypted = Some(false);
    reading.xlm = Some(xlm_of(package.macro_sheets));
    if let Some(vba_project) = package.vba_project {
        match CfbFile::open(&vba_project) {
            Ok(mut compound) => reading.read_project(&mut compound, OOXML_PROJECT_STORAGE),
            Err(error) => reading.failures.push(error.into()),
        }
    }
    reading
}

/// A compound file: an encrypted OOXML package, which is not opened, or a
/// BIFF8 workbook, whose `Workbook` stream gives its macro sheets and whether
/// it is encrypted (a FILEPASS record), and whose VBA project, which that
/// encryption does not reach, is read as usual.
///
/// A compound file with neither the stream nor the VBA storage is another kind
/// of file (a bare `vbaProject.bin`, a Word document). One with the storage is
/// a workbook that lost its stream.
fn read_compound(bytes: &[u8], extension: &str) -> Reading {
    let mut compound = match CfbFile::open(bytes) {
        Ok(compound) => compound,
        // It is not known which family the file belongs to.
        Err(error) => {
            let container = detect::container_by_extension(extension);
            return Reading::failed(container, error.into());
        }
    };
    if compound.is_encrypted_package() {
        let message = "the package is encrypted, and the tool does not decrypt";
        let failure = Failure::new(Status::Encrypted, "file.encrypted_package", message);
        return Reading::failed(detect::ooxml_container(extension), failure);
    }
    let container = detect::biff_container(extension);
    let mut reading = Reading::new(container);
    let has_project = compound.has_storage(BIFF8_PROJECT_STORAGE);
    match compound.read_stream(BIFF8_WORKBOOK_STREAM) {
        Ok(Some(stream)) => reading.read_workbook(&stream),
        Ok(None) if has_project => {
            let message = "the workbook has no Workbook stream";
            let failure = Failure::new(Status::Corrupted, "biff.workbook.missing", message);
            reading.failures.push(failure);
        }
        Ok(None) => {
            let failure = unsupported_format("the compound file has no Workbook stream");
            return Reading::failed(container, failure);
        }
        Err(error) => reading.failures.push(error.into()),
    }
    if has_project {
        reading.read_project(&mut compound, BIFF8_PROJECT_STORAGE);
    }
    reading
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_the_way_sha256_does() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
