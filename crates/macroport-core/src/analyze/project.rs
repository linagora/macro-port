// SPDX-License-Identifier: Apache-2.0
//! The VBA project of a compound file: its `dir` stream, its `PROJECT` stream,
//! then the source of each module (SPEC M1, MS-OVBA).

use super::{Failure, sha256_hex};
use crate::container::{CfbFile, ContainerError};
use crate::ovba::{
    DirModule, MAX_PROJECT_STREAM, ProjectInfo, module_source, parse_compressed_dir, parse_project,
};
use crate::report::{Metrics, ModuleReport, Status, VbaProject};

/// The most bytes that the source of one module may decompress to.
const MAX_MODULE_SOURCE: usize = 8 * 1024 * 1024;

/// The most bytes that the sources of a project may total, once decoded. As
/// decoding can triple a source, a module is read with a cap of a third of
/// what is left, and none once less than three bytes are left.
///
/// SPEC M1 keeps the peak memory of a file under 256 MB. Reading a project
/// adds the `vbaProject.bin` of a package and the stream being read (64 MB
/// each, the caps of the container layer), the sources kept (this limit) and
/// the working set of [`module_source`], seven times its cap: 64 + 64 + 32 +
/// 7 x 8 = 216 MB. The bytes of the file, which the caller holds, come on top.
const MAX_PROJECT_SOURCE: usize = 32 * 1024 * 1024;

/// The longest compiled part of a module, in bytes before its `TextOffset`,
/// that is trivial: a module whose source is empty and whose compiled part is
/// longer is suspected of having lost its source (SPEC M1 step 8).
///
/// SPEC gives 512, but the compiled part of an empty module that Excel writes
/// is 819 or 827 bytes, so 512 would flag all of them. ADR-0005 has the
/// measurements.
const TRIVIAL_COMPILED_PART: u32 = 1024;

/// A VBA project as the report shows it, with the source of its modules.
pub(super) struct Project {
    pub(super) report: VbaProject,
    /// The source of each module of `report`, in the same order.
    pub(super) sources: Vec<Option<String>>,
}

impl Project {
    /// Whether any module holds code.
    pub(super) fn has_code(&self) -> bool {
        self.sources
            .iter()
            .flatten()
            .any(|source| holds_code(source))
    }

    /// Whether any module is suspected of having lost its source.
    pub(super) fn has_source_missing(&self) -> bool {
        (self.report.modules.iter()).any(|module| module.source_missing_suspected == Some(true))
    }
}

/// The storage of a compound file that holds a VBA project.
struct Storage<'a, 'b> {
    compound: &'a mut CfbFile<'b>,
    /// Where it is in the compound file: empty for the root.
    path: &'a str,
}

impl Storage<'_, '_> {
    /// The stream at `path` below the storage, if it holds one: `PROJECT`, or
    /// `VBA/dir` for a stream of the `VBA` storage.
    fn stream(&mut self, path: &str) -> Result<Option<Vec<u8>>, ContainerError> {
        self.compound.read_stream(&format!("{}/{path}", self.path))
    }
}

/// Reads the VBA project in the storage at `storage_path` of a compound file:
/// the root of the `vbaProject.bin` of a package, `_VBA_PROJECT_CUR` in an
/// `.xls`.
///
/// A failure to read the `dir` stream is the failure of the project. A stream
/// or a module that cannot be read does not stop it: its failure is returned
/// with the project, and a module whose source is lost is listed without it.
pub(super) fn read(
    compound: &mut CfbFile<'_>,
    storage_path: &str,
) -> Result<(Project, Vec<Failure>), Failure> {
    let mut storage = Storage {
        compound,
        path: storage_path,
    };
    let Some(dir) = storage.stream("VBA/dir")? else {
        return Err(Failure::new(
            Status::Corrupted,
            "ovba.dir.missing",
            "the VBA storage has no dir stream",
        ));
    };
    let info = parse_compressed_dir(&dir)?;
    drop(dir);
    let mut failures = Vec::new();
    let properties = read_properties(&mut storage, info.code_page, &mut failures);
    let mut allowance = Allowance::new();
    let mut modules = Vec::new();
    let mut sources = Vec::new();
    for module in &info.modules {
        let outcome = allowance
            .read_next(|max_source| read_source(&mut storage, module, info.code_page, max_source));
        let source = match outcome {
            Ok(source) => Some(source),
            Err(failure) => {
                failures.push(failure.in_module(&module.name));
                None
            }
        };
        modules.push(module_report(module, &properties, source.as_deref()));
        sources.push(source);
    }
    let report = VbaProject {
        name: info.project_name,
        protected: properties.protected,
        code_page: info.code_page,
        references: Some(info.references),
        modules,
    };
    Ok((Project { report, sources }, failures))
}

/// What the `PROJECT` stream says. A project without that stream, or whose
/// stream cannot be read (a failure too), is taken for an unprotected one
/// whose modules are documents unless the `dir` stream says they are
/// procedural.
fn read_properties(
    storage: &mut Storage<'_, '_>,
    code_page: Option<u16>,
    failures: &mut Vec<Failure>,
) -> ProjectInfo {
    let stream = match storage.stream("PROJECT") {
        Ok(stream) => stream.unwrap_or_default(),
        Err(error) => {
            failures.push(error.into());
            Vec::new()
        }
    };
    failures.extend(oversized(&stream));
    parse_project(&stream, code_page)
}

/// The failure of a `PROJECT` stream above [`MAX_PROJECT_STREAM`], whose part
/// past the limit is not read.
fn oversized(stream: &[u8]) -> Option<Failure> {
    (stream.len() > MAX_PROJECT_STREAM).then(|| {
        Failure::new(
            Status::TooLarge,
            "ovba.project.too_large",
            format!("the PROJECT stream is over {MAX_PROJECT_STREAM} bytes"),
        )
    })
}

/// What the sources of a project may still take, and whether they are still
/// read.
struct Allowance {
    /// The bytes that the sources may still take, once decoded.
    left: usize,
    /// A module decompressed past its limit. The `dir` stream may name one
    /// stream for every module, so the modules after it are not read.
    stopped: bool,
}

impl Allowance {
    fn new() -> Self {
        Self {
            left: MAX_PROJECT_SOURCE,
            stopped: false,
        }
    }

    /// Reads the next source with `fetch`, which is handed the most bytes that
    /// the source may decompress to: a third of what is left, as decoding can
    /// triple a source, and at most [`MAX_MODULE_SOURCE`]. Nothing is read
    /// once the project has taken its share, or after a module that was
    /// suspicious.
    fn read_next(
        &mut self,
        fetch: impl FnOnce(usize) -> Result<String, Failure>,
    ) -> Result<String, Failure> {
        if self.stopped {
            return Err(Failure::new(
                Status::SuspiciousArchive,
                "ovba.module.not_read",
                "the module is not read: an earlier one decompressed past its limit",
            ));
        }
        let max_source = (self.left / 3).min(MAX_MODULE_SOURCE);
        if max_source == 0 {
            return Err(Failure::new(
                Status::SuspiciousArchive,
                "ovba.decompress.too_large",
                format!("the sources of the project take more than {MAX_PROJECT_SOURCE} bytes"),
            ));
        }
        let source = fetch(max_source);
        match &source {
            Ok(source) => self.left = self.left.saturating_sub(source.len()),
            Err(failure) => self.stopped |= failure.status == Status::SuspiciousArchive,
        }
        source
    }
}

/// The source of a module, read from its stream, which decompresses to at
/// most `max_source` bytes.
fn read_source(
    storage: &mut Storage<'_, '_>,
    module: &DirModule,
    code_page: Option<u16>,
    max_source: usize,
) -> Result<String, Failure> {
    let Some(stream) = storage.stream(&format!("VBA/{}", module.stream_name))? else {
        return Err(Failure::new(
            Status::Corrupted,
            "ovba.module.missing_stream",
            "the stream of the module is not in the VBA storage",
        ));
    };
    Ok(module_source(
        &stream,
        module.text_offset,
        code_page,
        max_source,
    )?)
}

/// Whether `source` holds code: a line that is neither blank, nor a comment,
/// nor an `Attribute` or `Option` line, which only describe the module.
///
/// What extraction alone can make of the table of SPEC M1, which gives `ok` to
/// a project with a procedure and `no_macros` to one of empty modules, and says
/// nothing of a module of declarations only: any code makes a project `ok`.
/// [`analyze`](super::analyze) replaces this guess with the procedures that the
/// front end finds.
fn holds_code(source: &str) -> bool {
    first_words(source).any(|word| {
        let described = ["rem", "attribute", "option"]
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword));
        !word.is_empty() && !word.starts_with('\'') && !described
    })
}

/// Whether a module has lost its source, as VBA stomping does (SPEC M1 step 8):
/// its `source` is empty, or holds blank lines and `Attribute` lines only,
/// while its compiled part, the `compiled` bytes before its `TextOffset`, is
/// not trivial.
fn source_missing(compiled: u32, source: &str) -> bool {
    compiled > TRIVIAL_COMPILED_PART
        && first_words(source).all(|word| word.is_empty() || word.eq_ignore_ascii_case("attribute"))
}

/// The first word of each line of `source`: empty for a blank line.
fn first_words(source: &str) -> impl Iterator<Item = &str> {
    (source.lines()).map(|line| line.split_whitespace().next().unwrap_or_default())
}

/// The report of a module whose source is `source`, or could not be read. Its
/// metrics and procedures are those of a module that has not been parsed.
///
/// Whether the source of the module is missing is unknown without a source.
fn module_report(
    module: &DirModule,
    properties: &ProjectInfo,
    source: Option<&str>,
) -> ModuleReport {
    ModuleReport {
        name: module.name.clone(),
        module_type: properties.module_type(module),
        stream_name: Some(module.stream_name.clone()),
        source_available: source.is_some(),
        source_missing_suspected: source.map(|source| source_missing(module.text_offset, source)),
        source_sha256: source.map(|source| sha256_hex(source.as_bytes())),
        metrics: Metrics::default(),
        declarations: None,
        procedures: Vec::new(),
        signals: None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn failure(status: Status) -> Failure {
        Failure::new(status, "ovba.test", "a failure")
    }

    #[test]
    fn a_source_is_read_with_a_third_of_what_is_left_up_to_the_limit_of_a_module() {
        let handed = Cell::new(0_usize);
        let read_four_bytes = |max_source| {
            handed.set(max_source);
            Ok("abcd".to_owned())
        };

        let mut allowance = Allowance::new();
        assert!(allowance.read_next(read_four_bytes).is_ok());
        assert_eq!(
            (handed.get(), allowance.left),
            (MAX_MODULE_SOURCE, MAX_PROJECT_SOURCE - 4)
        );

        let mut allowance = Allowance {
            left: 10,
            stopped: false,
        };
        assert!(allowance.read_next(read_four_bytes).is_ok());
        assert_eq!((handed.get(), allowance.left), (3, 6));
    }

    #[test]
    fn nothing_is_read_once_the_project_has_taken_its_share() {
        let reads = Cell::new(0_u32);
        let mut allowance = Allowance {
            left: 2,
            stopped: false,
        };

        let outcome = allowance.read_next(|_| {
            reads.set(reads.get().saturating_add(1));
            Ok(String::new())
        });

        let code = outcome.err().map(|failure| failure.error.code);
        assert_eq!(code.as_deref(), Some("ovba.decompress.too_large"));
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn a_suspicious_module_stops_the_reading_and_another_failure_does_not() {
        let reads = Cell::new(0_u32);
        let fail_with = |allowance: &mut Allowance, status| {
            allowance.read_next(|_| {
                reads.set(reads.get().saturating_add(1));
                Err(failure(status))
            })
        };
        let mut allowance = Allowance::new();

        let failures = [
            Status::Corrupted,
            Status::SuspiciousArchive,
            Status::Corrupted,
        ]
        .map(|status| fail_with(&mut allowance, status).expect_err("a failure"));

        // Two reads happen, and each failure is that of its read. The third
        // module is not read: the one before it was suspicious.
        assert_eq!(reads.get(), 2);
        let found = failures
            .each_ref()
            .map(|failure| (failure.status, failure.error.code.as_str()));
        let expected = [
            (Status::Corrupted, "ovba.test"),
            (Status::SuspiciousArchive, "ovba.test"),
            (Status::SuspiciousArchive, "ovba.module.not_read"),
        ];
        assert_eq!(found, expected);
    }

    #[test]
    fn a_project_stream_above_the_limit_is_a_failure_and_one_at_the_limit_is_not() {
        assert!(oversized(&vec![b' '; MAX_PROJECT_STREAM]).is_none());

        let failure = oversized(&vec![b' '; MAX_PROJECT_STREAM + 1]).expect("a failure");

        assert_eq!(failure.status, Status::TooLarge);
        assert_eq!(failure.error.code, "ovba.project.too_large");
    }

    #[test]
    fn code_is_what_is_neither_blank_nor_a_comment_nor_a_module_setting() {
        for source in [
            "",
            " \n",
            "' c\n Rem r\n",
            "Attribute VB_Name = \"A\"\n option base 1\n",
        ] {
            assert!(!holds_code(source), "{source:?}");
        }
        for source in ["Sub A()\nEnd Sub", "Const X = 1", "Remove = 1"] {
            assert!(holds_code(source), "{source:?}");
        }
    }

    #[test]
    fn a_source_is_missing_when_it_is_empty_and_the_compiled_part_is_not_trivial() {
        let attributes = "Attribute VB_Name = \"A\"\r\n\n  attribute VB_Base = \"0\"\n";
        for source in ["", "\n \n", attributes] {
            assert!(!source_missing(TRIVIAL_COMPILED_PART, source), "{source:?}");
            assert!(
                source_missing(TRIVIAL_COMPILED_PART + 1, source),
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_source_with_anything_else_is_not_missing_whatever_the_compiled_part() {
        for source in [
            "Sub A()\nEnd Sub",
            "' c",
            "Option Explicit",
            "Attributes = 1",
        ] {
            assert!(!source_missing(u32::MAX, source), "{source:?}");
        }
    }
}
