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
    let mut budget = MAX_PROJECT_SOURCE;
    let mut modules = Vec::new();
    let mut sources = Vec::new();
    for module in &info.modules {
        let source = match read_source(&mut storage, module, info.code_page, budget) {
            Ok(source) => {
                budget = budget.saturating_sub(source.len());
                Some(source)
            }
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

/// The source of a module, read from its stream. `budget` is what the sources
/// of the project may still take, once decoded.
fn read_source(
    storage: &mut Storage<'_, '_>,
    module: &DirModule,
    code_page: Option<u16>,
    budget: usize,
) -> Result<String, Failure> {
    let max_source = (budget / 3).min(MAX_MODULE_SOURCE);
    if max_source == 0 {
        // Not even read: the `dir` stream may name this stream for every module.
        return Err(Failure::new(
            Status::SuspiciousArchive,
            "ovba.decompress.too_large",
            format!("the sources of the project take more than {MAX_PROJECT_SOURCE} bytes"),
        ));
    }
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
    source.lines().any(|line| {
        let word = line.split_whitespace().next().unwrap_or_default();
        let described = ["rem", "attribute", "option"]
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword));
        !word.is_empty() && !word.starts_with('\'') && !described
    })
}

/// The report of a module whose source is `source`, or could not be read. Its
/// metrics and procedures are those of a module that has not been parsed.
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
        source_missing_suspected: None,
        source_sha256: source.map(|source| sha256_hex(source.as_bytes())),
        metrics: Metrics::default(),
        declarations: None,
        procedures: Vec::new(),
        signals: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
