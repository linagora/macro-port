// SPDX-License-Identifier: Apache-2.0
//! The modules of a file, from their source to their report (SPEC M2).
//!
//! The front end reads each module on its own, from its text alone. [`Analysis`]
//! holds what it makes of one: the lines, and the procedures that the parser
//! finds in them. [`Front`] puts that into the report of the module, and
//! completes the report of the file once every module is read. [`finish`] runs
//! it on the modules of an extraction: those that were found in a workbook, or
//! the text modules that [`analyze_modules`] (in the `text` submodule) wraps in
//! one.
//!
//! The file is `ok` when a module has a procedure, when it has Excel 4.0 macro
//! sheets or when the encryption of the workbook hides its sheets, and
//! `no_macros` when none of these holds, the project being described all the
//! same. A module that cannot be read keeps the report of a module that no stage
//! measured, and its failure gives the file its status.
//!
//! Only what the parser shows is filled so far: the spans and the sizes of the
//! procedures, and the trait that the recoveries of the parser give. The other
//! fields are left out until the stages that measure them run, since a missing
//! value is not a zero.
//!
//! Nothing here executes or evaluates anything from the file.

mod text;

use super::{Extraction, Failure, Options, readable_status};
use crate::report::{
    FileReport, Metrics, ModuleReport, Orientation, ProcedureReport, Status, VbaProject,
};
use crate::rules::{ModuleType, RuleSet, StructuralPredicate, TraitRule};
use crate::vba::{
    LogicalKind, LogicalLine, ParsedModule, Procedure, SourceLines, VbaError, parse, split_lines,
};

pub use self::text::analyze_modules;

/// The family of a procedure that no rule has scored: the rule engine (M3)
/// classifies the procedures, so until it runs none is.
const UNCLASSIFIED: &str = "unclassified";

/// The most lines that the modules of one file may total, once read.
/// [`split_lines`] bounds one module, and the sources of a project may total
/// 32 MiB, which a module of one-character lines turns into far more lines than
/// the time and the report of a file should take. The modules past this budget
/// are not read.
const MAX_FILE_LINES: u32 = 1_000_000;

/// What the front end makes of the text of one module.
pub(super) struct Analysis {
    /// Its physical lines, the lines that are not code, and its logical lines.
    pub(super) lines: SourceLines,
    /// Its procedures, and where the parser stepped over something.
    pub(super) module: ParsedModule,
}

impl Analysis {
    /// Reads `text`. Fails only on a text of too many lines.
    fn of(text: &str) -> Result<Self, VbaError> {
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

/// The front end at work on the modules of one file.
struct Front {
    /// The lines that the modules may still take.
    budget: u32,
    /// The ids of the traits that the rules give to the predicate
    /// `parse_recovered`. The parser decides that one, so the procedures and
    /// the file that have it can be marked before the rule engine runs.
    recovery_traits: Vec<String>,
    /// Whether the parser stepped over something in a module read so far.
    recovered: bool,
    /// Why modules were not read, in the order they were met.
    failures: Vec<Failure>,
}

impl Front {
    fn new(rules: &RuleSet) -> Self {
        Self {
            budget: MAX_FILE_LINES,
            recovery_traits: recovery_traits(rules),
            recovered: false,
            failures: Vec::new(),
        }
    }

    /// Reads `text`, the source of `module`, into its report. A module that
    /// cannot be read keeps the report that extraction gave it, and its failure
    /// is kept for the file.
    fn read(&mut self, module: &mut ModuleReport, text: &str) {
        match self.analyse(text) {
            Ok(analysis) => self.fill(module, &analysis),
            Err(error) => (self.failures).push(Failure::from(error).in_module(&module.name)),
        }
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

    /// Puts what `analysis` shows of a module into its report: the sizes of the
    /// module and its procedures with their spans.
    fn fill(&mut self, module: &mut ModuleReport, analysis: &Analysis) {
        let lines = &analysis.lines;
        module.procedures = (analysis.module.procedures.iter())
            .map(|procedure| procedure_report(procedure, lines, &self.recovery_traits))
            .collect();
        module.metrics = Metrics {
            loc: lines.physical_lines,
            logical_loc: logical_lines(&lines.logical),
            procedures: Some(count(module.procedures.len())),
            ..Metrics::default()
        };
        self.recovered |= analysis.module.is_recovered();
    }

    /// Completes the report of a file once its modules are read: the metrics of
    /// the file sum those of its modules, the errors of the front end join those
    /// of the extraction, and the status follows from the procedures.
    ///
    /// A file that extraction could not read keeps the status of its first
    /// failure. Otherwise the first failure of the front end gives the status,
    /// and without one the file is `ok` if a module has a procedure, or the file
    /// has macro sheets or a workbook whose encryption hides them, and
    /// `no_macros` if not.
    fn seal(self, file: &mut FileReport) {
        if let Some(project) = &file.vba_project {
            file.metrics = Some(file_metrics(project));
        }
        if matches!(file.status, Status::Ok | Status::NoMacros) {
            file.status = match self.failures.first() {
                Some(first) => first.status,
                None => {
                    let procedures = (file.vba_project.iter())
                        .flat_map(|project| &project.modules)
                        .any(|module| !module.procedures.is_empty());
                    readable_status(procedures, file.xlm.as_ref(), file.workbook_encrypted)
                }
            };
        }
        if self.recovered && !self.recovery_traits.is_empty() {
            file.traits = Some(self.recovery_traits);
        }
        let errors = file.errors.get_or_insert_with(Vec::new);
        errors.extend(self.failures.into_iter().map(|failure| failure.error));
    }
}

/// Reads the modules that extraction found, and completes the report of the
/// file.
pub(super) fn finish(extraction: Extraction, options: &Options<'_>) -> FileReport {
    let Extraction { mut file, sources } = extraction;
    let mut front = Front::new(options.rules);
    if let Some(project) = file.vba_project.as_mut() {
        for (module, source) in project.modules.iter_mut().zip(&sources) {
            if let Some(text) = source {
                front.read(module, text);
            }
        }
    }
    front.seal(&mut file);
    file
}

/// The ids of the traits that the rules give to the predicate `parse_recovered`.
fn recovery_traits(rules: &RuleSet) -> Vec<String> {
    let recovered = |rule: &TraitRule| {
        matches!(
            rule,
            TraitRule::Structural(StructuralPredicate::ParseRecovered)
        )
    };
    (rules.complexity.traits.iter())
        .filter(|candidate| recovered(&candidate.rule))
        .map(|candidate| candidate.id.clone())
        .collect()
}

/// The report of a procedure, as far as the structure of the module shows it.
fn procedure_report(
    procedure: &Procedure,
    lines: &SourceLines,
    recovery_traits: &[String],
) -> ProcedureReport {
    let recovered = procedure.is_recovered() && !recovery_traits.is_empty();
    ProcedureReport {
        name: procedure.name.clone(),
        kind: procedure.kind,
        visibility: Some(procedure.visibility),
        is_entry_point: None,
        is_event_handler: None,
        possibly_unused: None,
        start_line: procedure.start_line,
        end_line: procedure.end_line,
        metrics: procedure_metrics(procedure, lines),
        calls: None,
        apis: None,
        primary_family: UNCLASSIFIED.to_owned(),
        secondary_families: None,
        families: Vec::new(),
        signals: Vec::new(),
        traits: recovered.then(|| recovery_traits.to_vec()),
        orientation: Orientation,
    }
}

/// The metrics of a procedure.
fn procedure_metrics(procedure: &Procedure, lines: &SourceLines) -> Metrics {
    let code = (lines.logical.get(procedure.lines.clone())).unwrap_or_default();
    Metrics {
        loc: (procedure.end_line.saturating_sub(procedure.start_line)).saturating_add(1),
        logical_loc: logical_lines(code),
        parameters: Some(count(procedure.parameter_count)),
        ..Metrics::default()
    }
}

/// The number of logical lines that are not comment-only.
fn logical_lines(lines: &[LogicalLine]) -> u32 {
    let code = (lines.iter()).filter(|line| line.kind != LogicalKind::CommentOnly);
    count(code.count())
}

/// A length as the `u32` that the report holds: `u32::MAX` if it is larger.
fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The metrics of a file: the sizes of its modules summed.
fn file_metrics(project: &VbaProject) -> Metrics {
    let mut metrics = Metrics::default();
    let (mut procedures, mut forms) = (0u32, 0u32);
    for module in &project.modules {
        metrics.loc = metrics.loc.saturating_add(module.metrics.loc);
        metrics.logical_loc = (metrics.logical_loc).saturating_add(module.metrics.logical_loc);
        procedures = procedures.saturating_add(count(module.procedures.len()));
        forms = forms.saturating_add(u32::from(module.module_type == ModuleType::Form));
    }
    metrics.procedures = Some(procedures);
    metrics.forms = Some(forms);
    metrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RulesSources;

    fn front() -> Front {
        let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
        Front::new(&rules)
    }

    #[test]
    fn the_lines_of_a_file_are_read_within_a_budget() {
        let mut front = front();
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
