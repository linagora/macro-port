// SPDX-License-Identifier: Apache-2.0
//! The VBA project of a compound file: its `dir` stream, its `PROJECT` stream,
//! then the source of each module (SPEC M1, MS-OVBA).

use super::{DEFAULT_MAX_FILE_SIZE, Failure, sha256_hex};
use crate::container::{CfbFile, MAX_STREAM_SIZE};
use crate::ovba::{
    DirModule, MAX_PROJECT_STREAM, ProjectInfo, module_source, parse_compressed_dir, parse_project,
};
use crate::report::{Metrics, ModuleReport, Status, VbaProject};

/// The most bytes that the source of one module may decompress to.
const MAX_MODULE_SOURCE: usize = 8 * 1024 * 1024;

/// The most bytes that the modules of a project may take in all: the sources
/// kept, once decoded, and the output of the reads that fail, which is thrown
/// away but was decompressed. As decoding can triple a source, a module is read
/// with a cap of a third of what is left, and none once less than three bytes
/// are left. Charging the reads that fail bounds the output that a project
/// decompresses as well as the memory it keeps: whatever the number of modules
/// and the streams they name, that output is a small multiple of this limit.
/// What a stream costs apart from its output is not charged here: the copy
/// that each module makes of it, and the scan of the chunks that give little
/// or no output. [`MAX_PROJECT_READ`] bounds them, as far as they follow the
/// length of the stream.
///
/// SPEC M1 keeps the peak memory of a file under 256 MB. Reading a project
/// adds the `vbaProject.bin` of a package and the stream being read (64 MB
/// each, the caps of the container layer), the sources kept (this limit) and
/// the working set of [`module_source`], seven times its cap: 64 + 64 + 32 +
/// 7 x 8 = 216 MB. The bytes of the file, which the caller holds, come on top.
const MAX_PROJECT_SOURCE: usize = 32 * 1024 * 1024;

/// The most bytes that a project may read from its streams in all: the `dir`
/// and `PROJECT` streams and the stream of each module, each counted at the
/// length its directory entry gives, every time it is read. The `dir` stream
/// may name one stream for every module, and a read copies the whole stream
/// and decodes the part after the text offset, however little that gives:
/// 2,000 modules (`MAX_MODULES`) that name a stream of 64 MiB
/// ([`MAX_STREAM_SIZE`]) would copy 2,000 x 64 MiB = 125 GiB. A stream is
/// counted before it is read, so a read that fails after it copied bytes is
/// counted too.
///
/// The streams of an honest compound file take distinct sectors, and an honest
/// project names each of its streams once, so it reads at most the bytes of its
/// compound file: the file itself for an `.xls`, 200 MiB by default
/// ([`DEFAULT_MAX_FILE_SIZE`]), or the VBA part of a package, 64 MiB at most.
/// This limit is the larger of the two, so with the default limit of file size
/// that project is never refused. The project above is stopped after three
/// reads of the largest stream: 3 x 64 = 192 MiB, and a fourth would make
/// 256 MiB.
///
/// A stream above [`MAX_STREAM_SIZE`] is refused by the container before it
/// reads a byte: it is not counted, and keeps its status, `too_large`. What the
/// streams decode to is bounded apart, by [`MAX_PROJECT_SOURCE`].
///
/// The length is the file's claim, and what a read costs may not follow it:
/// `cfb` 0.15.0 walks the whole sector chain of a stream of the mini stream
/// at least twice per read, and the chain of a stream that claims 100 bytes
/// can be millions of mini sectors long. That cost is not bounded here.
const MAX_PROJECT_READ: u64 = DEFAULT_MAX_FILE_SIZE;

// One stream of the longest length that the container reads must fit, or an
// honest project would be refused.
const _: () = assert!(MAX_PROJECT_READ >= MAX_STREAM_SIZE);

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

/// What the streams of a project may still be read for (see
/// [`MAX_PROJECT_READ`]).
struct StreamMeter {
    /// The bytes that the streams may still take.
    left: u64,
}

impl StreamMeter {
    fn new() -> Self {
        Self {
            left: MAX_PROJECT_READ,
        }
    }

    /// Takes the `len` bytes of a stream that is about to be read from what is
    /// left. A stream that is longer than what is left is not taken, and must
    /// not be read: the failure is suspicious.
    fn charge(&mut self, len: u64) -> Result<(), Failure> {
        let left = self.left.checked_sub(len).ok_or_else(|| {
            Failure::new(
                Status::SuspiciousArchive,
                "ovba.project.streams_too_large",
                format!("the streams of the project add up to more than {MAX_PROJECT_READ} bytes"),
            )
        })?;
        self.left = left;
        Ok(())
    }
}

/// The storage of a compound file that holds a VBA project, and what its
/// streams may still be read for.
struct Storage<'a, 'b> {
    compound: &'a mut CfbFile<'b>,
    /// Where it is in the compound file: empty for the root.
    path: &'a str,
    meter: StreamMeter,
}

impl Storage<'_, '_> {
    /// The stream at `path` below the storage, if it holds one: `PROJECT`, or
    /// `VBA/dir` for a stream of the `VBA` storage.
    ///
    /// The length that the directory gives the stream is counted before it is
    /// read, whether the read succeeds or fails: a stream that would take the
    /// project past [`MAX_PROJECT_READ`] is not read.
    fn stream(&mut self, path: &str) -> Result<Option<Vec<u8>>, Failure> {
        let path = format!("{}/{path}", self.path);
        // The container refuses a stream above its cap before it reads a byte:
        // that costs nothing, and the failure is its own.
        let len = (self.compound.stream_len(&path)).filter(|&len| len <= MAX_STREAM_SIZE);
        self.meter.charge(len.unwrap_or_default())?;
        Ok(self.compound.read_stream(&path)?)
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
        meter: StreamMeter::new(),
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
        Err(failure) => {
            failures.push(failure);
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

/// What the modules of a project may still take (see [`MAX_PROJECT_SOURCE`]),
/// and whether they are still read.
struct Allowance {
    /// The bytes that the modules may still take.
    left: usize,
    /// A module went past a limit: it decompressed past its own, or its
    /// stream took the project past [`MAX_PROJECT_READ`]. The `dir` stream may
    /// name one stream for every module, so the modules after it are not read.
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
    /// triple a source, and at most [`MAX_MODULE_SOURCE`]. The read takes from
    /// what is left the length of the source it gives or, if it fails, the
    /// bytes it had decompressed. Nothing is read once the project has taken
    /// its share, or after a module that was suspicious.
    fn read_next(
        &mut self,
        fetch: impl FnOnce(usize) -> Result<String, Failure>,
    ) -> Result<String, Failure> {
        if self.stopped {
            return Err(Failure::new(
                Status::SuspiciousArchive,
                "ovba.module.not_read",
                "the module is not read: an earlier one went past a limit",
            ));
        }
        let max_source = (self.left / 3).min(MAX_MODULE_SOURCE);
        if max_source == 0 {
            return Err(Failure::new(
                Status::SuspiciousArchive,
                "ovba.decompress.too_large",
                format!("the modules of the project take more than {MAX_PROJECT_SOURCE} bytes"),
            ));
        }
        let source = fetch(max_source);
        match &source {
            Ok(source) => self.left = self.left.saturating_sub(source.len()),
            Err(failure) => {
                self.left = self.left.saturating_sub(failure.produced);
                self.stopped |= failure.status == Status::SuspiciousArchive;
            }
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
    use crate::ovba::MAX_MODULES;

    fn failure(status: Status) -> Failure {
        Failure::new(status, "ovba.test", "a failure")
    }

    /// The failure of a read that had decompressed `produced` bytes.
    fn failure_after(status: Status, produced: usize) -> Failure {
        Failure {
            produced,
            ..failure(status)
        }
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
    fn a_read_that_fails_takes_the_bytes_it_had_decompressed_from_what_is_left() {
        let mut allowance = Allowance {
            left: 100,
            stopped: false,
        };

        let _ = allowance.read_next(|_| Err(failure_after(Status::Corrupted, 40)));
        assert_eq!((allowance.left, allowance.stopped), (60, false));

        let _ = allowance.read_next(|_| Err(failure_after(Status::Corrupted, 1_000)));
        assert_eq!((allowance.left, allowance.stopped), (0, false));
    }

    #[test]
    fn modules_that_break_after_all_they_may_take_use_up_the_budget_in_a_few_reads() {
        // Each read decompresses all that it is allowed to, then breaks: what a
        // stream does that is made to, and that every module may name.
        let mut allowance = Allowance::new();
        let (mut reads, mut produced) = (0_u32, 0_usize);
        let mut last = None;

        for _ in 0..MAX_MODULES {
            last = allowance
                .read_next(|max_source| {
                    reads = reads.saturating_add(1);
                    produced = produced.saturating_add(max_source);
                    Err(failure_after(Status::Corrupted, max_source))
                })
                .err();
        }

        // 42 reads, and not 2,000 of 8 MiB each.
        assert!(reads <= 64, "{reads} reads");
        assert!(produced <= MAX_PROJECT_SOURCE, "{produced} bytes");
        // No read was suspicious, so the reading was not stopped: the modules
        // after the last read are refused for want of budget.
        assert!(!allowance.stopped);
        let last = last.expect("a failure");
        assert_eq!(
            (last.status, last.error.code.as_str()),
            (Status::SuspiciousArchive, "ovba.decompress.too_large")
        );
    }

    #[test]
    fn streams_are_taken_from_what_is_left_down_to_the_last_byte() {
        let mut meter = StreamMeter { left: 100 };

        assert!(meter.charge(40).is_ok());
        assert_eq!(meter.left, 60);
        assert!(meter.charge(60).is_ok());
        assert_eq!(meter.left, 0);

        // An empty stream, or none, costs nothing, whatever is left.
        assert!(meter.charge(0).is_ok());
        assert_eq!(meter.left, 0);
    }

    #[test]
    fn a_stream_longer_than_what_is_left_is_suspicious_and_is_not_taken() {
        let mut meter = StreamMeter { left: 100 };
        assert!(meter.charge(70).is_ok());

        let failure = meter.charge(31).expect_err("a failure");

        let found = (failure.status, failure.error.code.as_str());
        let expected = (Status::SuspiciousArchive, "ovba.project.streams_too_large");
        assert_eq!(found, expected);
        // That stream is not read, so a shorter one may still be.
        assert_eq!(meter.left, 30);
        assert!(meter.charge(30).is_ok());
    }

    #[test]
    fn modules_that_name_the_largest_stream_are_stopped_after_three_reads() {
        let mut meter = StreamMeter::new();

        let reads = (0..MAX_MODULES)
            .take_while(|_| meter.charge(MAX_STREAM_SIZE).is_ok())
            .count();

        // 3 x 64 MiB are under the 200 MiB of the limit, and 4 x 64 MiB are not.
        assert_eq!(reads, 3);
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
