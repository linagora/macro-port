// SPDX-License-Identifier: Apache-2.0
//! The modules of a file, from their source to their report (SPEC M2 and M3).
//!
//! The front end reads each module on its own, from its text alone. [`Analysis`]
//! holds what it makes of one: the lines, and the procedures that the parser
//! finds in them. [`Front`] puts that into the report of the module, keeps what
//! the rule engine reads of it, and once every module is read, runs the engine
//! on the project and completes the report of the file. Two doors lead here:
//!
//! - [`finish`] reads the modules that extraction found in a workbook;
//! - [`analyze_modules`] reads modules that the caller hands over as text.
//!
//! The second gives a file whose container is `text_module`, with the modules
//! as its VBA project (SPEC, "Text entry point"), and skips the container layer.
//!
//! The file is `ok` when a module has a procedure, when it has Excel 4.0 macro
//! sheets or when the encryption of the workbook hides its sheets, and
//! `no_macros` when none of these holds, the project being described all the
//! same. A module that cannot be read keeps the report of a module that no stage
//! measured, and its failure gives the file its status. The rule engine runs as
//! soon as one module is read, so that what can be read of a file is judged
//! whatever else went wrong with it.
//!
//! The container indicators that extraction raises are not passed to the engine
//! yet, and the members, chains, calls and ProgIDs of the statements are not
//! read, nor are the entry points and the call graph: what depends on them is
//! left out, since a missing value is not a zero (see [`input`](super::input)).
//!
//! Nothing here executes or evaluates anything from the file.

use sha2::{Digest, Sha256};

use super::apply::apply;
use super::input::{Adapted, Measured, adapt};
use super::metrics::{count, file_metrics, module_metrics, project_metrics};
use super::{Extraction, Failure, Options, Project, Reading, hex, readable_status, sha256_hex};
use crate::engine::{ModuleInput, ProjectInput, findings};
use crate::report::{
    Container, Declarations, FileReport, Metrics, ModuleReport, Orientation, ProcedureReport,
    Status, VbaProject,
};
use crate::rules::{ModuleType, RuleSet};
use crate::vba::{
    DeclarationKind, ModuleSource, OptionKind, ParsedModule, Procedure, SourceLines, VbaError,
    parse, split_lines,
};

/// The family of a procedure before the rule engine has scored it, and of the
/// procedures of a module that it did not run on.
const UNCLASSIFIED: &str = "unclassified";

/// The most lines that the modules of one file may total, once read.
/// [`split_lines`] bounds one module, and the sources of a project may total
/// 32 MiB, which a module of one-character lines turns into far more lines than
/// the time and the report of a file should take. The modules past this budget
/// are not read.
const MAX_FILE_LINES: u32 = 1_000_000;

/// The most bytes that the modules handed to [`analyze_modules`] may total: the
/// limit of the sources of a VBA project, which keeps the peak memory of a file
/// under 256 MB (SPEC M1). The limit of [`Options::max_file_size`] applies as
/// well when it is lower.
const MAX_TEXT_SIZE: u64 = 32 * 1024 * 1024;

/// What the front end makes of the text of one module.
pub(super) struct Analysis {
    /// Its physical lines, the lines that are not code, and its logical lines.
    pub(super) lines: SourceLines,
    /// Its procedures, and where the parser stepped over something.
    pub(super) module: ParsedModule,
}

impl Analysis {
    /// Reads `text`. Fails only on a text of too many lines.
    pub(super) fn of(text: &str) -> Result<Self, VbaError> {
        let lines = split_lines(text)?;
        let module = parse(&lines);
        Ok(Self { lines, module })
    }

    /// How many lines reading the module took: the greater of its physical and
    /// its logical lines, since a line can hold many statements.
    fn weight(&self) -> u32 {
        count(self.lines.logical.len()).max(self.lines.physical_lines)
    }
}

impl From<VbaError> for Failure {
    fn from(error: VbaError) -> Self {
        Self::new(error.status, error.code, error.message)
    }
}

/// The front end at work on the modules of one file.
struct Front<'r> {
    /// The rules that the engine applies once the modules are read.
    rules: &'r RuleSet,
    /// The lines that the modules may still take.
    budget: u32,
    /// What the rule engine reads of each module met so far, one entry for each,
    /// in order: the modules of the engine are those of the report.
    inputs: Vec<ModuleInput>,
    /// What was measured of each of them, in the same order.
    measured: Vec<Measured>,
    /// Why modules were not read, in the order they were met.
    failures: Vec<Failure>,
}

impl<'r> Front<'r> {
    fn new(rules: &'r RuleSet) -> Self {
        Self {
            rules,
            budget: MAX_FILE_LINES,
            inputs: Vec::new(),
            measured: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Reads `text`, the source of `module`, into its report: none for a module
    /// whose source could not be extracted. A module that cannot be read keeps
    /// the report that extraction gave it, and its failure is kept for the file.
    fn read(&mut self, module: &mut ModuleReport, text: Option<&str>) {
        let adapted = match text.map(|text| self.analyse(text)) {
            Some(Ok(analysis)) => Self::fill(module, &analysis),
            Some(Err(error)) => {
                (self.failures).push(Failure::from(error).in_module(&module.name));
                Adapted::unread(&module.name, module.module_type)
            }
            None => Adapted::unread(&module.name, module.module_type),
        };
        self.inputs.push(adapted.input);
        self.measured.push(adapted.measured);
    }

    /// Reads the next module. One that crosses the budget is read whole, and the
    /// ones after it are not read, but for the empty ones.
    fn analyse(&mut self, text: &str) -> Result<Analysis, VbaError> {
        if self.budget == 0 && !text.is_empty() {
            return Err(VbaError::new(
                Status::TooLarge,
                "vba.too_many_lines",
                format!("the modules of the file have more than {MAX_FILE_LINES} lines"),
            ));
        }
        let analysis = Analysis::of(text)?;
        self.budget = self.budget.saturating_sub(analysis.weight());
        Ok(analysis)
    }

    /// Puts what `analysis` shows of a module into its report: its sizes, its
    /// declarations, and its procedures with their spans and their sizes. What
    /// the rule engine decides is left for [`seal`](Self::seal).
    fn fill(module: &mut ModuleReport, analysis: &Analysis) -> Adapted {
        let adapted = adapt(&module.name, module.module_type, analysis);
        let metrics = &adapted.measured.procedures;
        module.procedures = (analysis.module.procedures.iter().zip(metrics))
            .map(|(procedure, metrics)| procedure_report(procedure, metrics))
            .collect();
        module.metrics = module_metrics(&analysis.lines, metrics);
        module.declarations = Some(declarations(&analysis.module));
        adapted
    }

    /// Completes the report of a file once its modules are read: the metrics of
    /// the file sum those of its modules, the rule engine decides the families,
    /// the signals, the traits and the level, the errors of the front end join
    /// those of the extraction, and the status follows from the procedures.
    ///
    /// The engine runs when a module was read, whatever the status of the file:
    /// a file of which some modules could not be read is judged on the others.
    ///
    /// A file that extraction could not read keeps the status of its first
    /// failure. Otherwise the first failure of the front end gives the status,
    /// and without one the file is `ok` if a module has a procedure, or the file
    /// has macro sheets or a workbook whose encryption hides them, and
    /// `no_macros` if not.
    fn seal(self, file: &mut FileReport) {
        let Self {
            rules,
            inputs,
            measured,
            failures,
            ..
        } = self;
        if let Some(report) = &file.vba_project {
            let references = report.references.clone().unwrap_or_default();
            let project = ProjectInput {
                references,
                modules: inputs,
            };
            let metrics = project_metrics(&project, &measured, file_metrics(report));
            file.metrics = Some(metrics.file.clone());
            if measured.iter().any(|module| module.read) {
                // The indicators of the container are not passed yet.
                let decided = findings::run(&project, rules, &metrics, &[]);
                apply(file, decided, metrics, &measured);
            }
        }
        if matches!(file.status, Status::Ok | Status::NoMacros) {
            file.status = match failures.first() {
                Some(first) => first.status,
                None => {
                    let procedures = (file.vba_project.iter())
                        .flat_map(|project| &project.modules)
                        .any(|module| !module.procedures.is_empty());
                    readable_status(procedures, file.xlm.as_ref(), file.workbook_encrypted)
                }
            };
        }
        let errors = file.errors.get_or_insert_with(Vec::new);
        errors.extend(failures.into_iter().map(|failure| failure.error));
    }
}

/// Reads the modules that extraction found in a workbook, and completes its
/// report.
pub(super) fn finish(extraction: Extraction, options: &Options<'_>) -> FileReport {
    let Extraction { mut file, sources } = extraction;
    let mut front = Front::new(options.rules);
    if let Some(project) = file.vba_project.as_mut() {
        for (at, module) in project.modules.iter_mut().enumerate() {
            let text = sources.get(at).and_then(|source| source.as_deref());
            front.read(module, text);
        }
    }
    front.seal(&mut file);
    file
}

/// Analyses VBA modules that the caller hands over as text: a file of
/// `container: "text_module"` whose VBA project holds them (SPEC, "Text entry
/// point"). It skips the container layer, and so reads no file.
///
/// `path` is the path as the report gives it. The `size` of the file is the
/// bytes of the texts and its `sha256` the hash of the texts one after the
/// other, which for one module is its text. Texts above 32 MiB in all, or above
/// [`Options::max_file_size`] when that is lower, are `too_large` and not
/// read. A module that cannot be read gets the failure that
/// [`analyze`](super::analyze) gives, and the other modules are read all the
/// same. The result never fails.
pub fn analyze_modules(
    modules: Vec<ModuleSource>,
    path: &str,
    options: &Options<'_>,
) -> FileReport {
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    for module in &modules {
        hasher.update(module.text.as_bytes());
        size = size.saturating_add(u64::try_from(module.text.len()).unwrap_or(u64::MAX));
    }
    let sha256 = hex(&hasher.finalize());
    if size > text_limit(options) {
        return too_large_text(path, size, sha256, options);
    }
    let (reports, sources): (Vec<_>, Vec<_>) = modules
        .into_iter()
        .map(|module| {
            let report = text_module(&module.name, module.module_type, &module.text);
            (report, Some(module.text))
        })
        .unzip();
    finish(
        text_extraction(path, size, sha256, reports, sources),
        options,
    )
}

/// The limit on the size of the modules of a text file: that of the options,
/// and [`MAX_TEXT_SIZE`].
fn text_limit(options: &Options<'_>) -> u64 {
    options.max_file_size.min(MAX_TEXT_SIZE)
}

/// The report of modules above the size limit. Like any file above it, they are
/// not read.
fn too_large_text(path: &str, size: u64, sha256: String, options: &Options<'_>) -> FileReport {
    let limited = Options {
        max_file_size: text_limit(options),
        ..*options
    };
    let mut file = super::too_large(path, size, sha256, &limited);
    file.container = Container::TextModule;
    file
}

/// The extraction of text modules: their reports, which the front end has not
/// filled, and their texts.
fn text_extraction(
    path: &str,
    size: u64,
    sha256: String,
    modules: Vec<ModuleReport>,
    sources: Vec<Option<String>>,
) -> Extraction {
    let report = VbaProject {
        name: None,
        protected: false,
        code_page: None,
        references: None,
        modules,
    };
    let mut reading = Reading::new(Container::TextModule);
    reading.project = Some(Project { report, sources });
    reading.into_extraction(path, size, sha256)
}

/// The report of a text module, as extraction would give it before the front
/// end reads it.
fn text_module(name: &str, module_type: ModuleType, text: &str) -> ModuleReport {
    ModuleReport {
        name: name.to_owned(),
        module_type,
        stream_name: None,
        source_available: true,
        source_missing_suspected: None,
        source_sha256: Some(sha256_hex(text.as_bytes())),
        metrics: Metrics::default(),
        declarations: None,
        procedures: Vec::new(),
        signals: None,
    }
}

/// The report of a procedure, as far as the structure of the module shows it,
/// given its `metrics`. The fields that the rule engine decides hold what a
/// procedure that no rule has scored has, until [`apply`] writes the findings.
fn procedure_report(procedure: &Procedure, metrics: &Metrics) -> ProcedureReport {
    ProcedureReport {
        name: procedure.name.clone(),
        kind: procedure.kind,
        visibility: Some(procedure.visibility),
        is_entry_point: None,
        is_event_handler: None,
        possibly_unused: None,
        start_line: procedure.start_line,
        end_line: procedure.end_line,
        metrics: metrics.clone(),
        calls: None,
        apis: None,
        primary_family: UNCLASSIFIED.to_owned(),
        secondary_families: None,
        families: Vec::new(),
        signals: Vec::new(),
        traits: None,
        orientation: Orientation,
    }
}

/// The declarations section of the report of a module.
fn declarations(module: &ParsedModule) -> Declarations {
    let (mut declares, mut with_events) = (0usize, 0usize);
    let (mut explicit, mut vba_support) = (false, false);
    for declaration in &module.declarations {
        match declaration.kind {
            DeclarationKind::Declare { .. } => declares = declares.saturating_add(1),
            DeclarationKind::WithEvents { .. } => with_events = with_events.saturating_add(1),
            DeclarationKind::Option(OptionKind::Explicit) => explicit = true,
            DeclarationKind::Option(OptionKind::VbaSupport) => vba_support = true,
            _ => {}
        }
    }
    Declarations {
        declare_count: Some(count(declares)),
        with_events_count: Some(count(with_events)),
        option_explicit: Some(explicit),
        option_vba_support: Some(vba_support),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RulesSources;

    fn rules() -> RuleSet {
        RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
    }

    #[test]
    fn the_lines_of_a_file_are_read_within_a_budget() {
        let rules = rules();
        let mut front = Front::new(&rules);
        front.budget = 10;
        let module = "a\n".repeat(6);

        assert!(front.analyse(&module).is_ok());
        // Four lines are left, and the module that crosses the budget is read.
        assert!(front.analyse(&module).is_ok());
        let error = front.analyse(&module).err().expect("over the budget");
        assert_eq!(
            (error.status, error.code),
            (Status::TooLarge, "vba.too_many_lines")
        );
        assert!(front.analyse("").is_ok());
    }

    #[test]
    fn a_module_weighs_its_physical_or_its_logical_lines() {
        let weight = |text| Analysis::of(text).expect("a short module").weight();

        assert_eq!(weight("a\nb\nc"), 3);
        assert_eq!(weight("a: b: c: d"), 4);
        assert_eq!(weight(""), 0);
    }
}
