// SPDX-License-Identifier: Apache-2.0
//! The modules of a file, from their source to their report (SPEC M2).
//!
//! The front end reads each module on its own, from its text alone. [`Analysis`]
//! holds what it makes of one: the lines, and the procedures that the parser
//! finds in them. [`Front`] puts that into the report of the module, and
//! completes the report of the file once every module is read. [`finish`] runs
//! it on the modules of an extraction, those that were found in a workbook. The
//! text modules have two doors of their own: [`analyze_modules`] (the `text`
//! submodule) wraps modules that the caller has as text in a file, and
//! `analyze_text` (the `text_file` submodule) reads a `.bas`, `.cls` or `.frm`
//! file as the one module of a file.
//!
//! The file is `ok` when a module has a procedure or is suspected of having lost
//! its source, when it has Excel 4.0 macro sheets or when the encryption of the
//! workbook hides its sheets, and `no_macros` when none of these holds, the
//! project being described all the same. A module that cannot be read keeps the
//! report of a module that no stage measured, and its failure gives the file its
//! status.
//!
//! Only what the parser shows is filled so far: the spans and the sizes of the
//! procedures, the declarations of the modules (and the count of their `Declare`
//! statements, a metric), and the trait that the recoveries of the parser give.
//! The other fields are left out until the stages that measure them run, since a
//! missing value is not a zero. The exceptions are three fields of a procedure
//! that the schema requires and that only the rule engine can decide: until it
//! runs, `primary_family` is `unclassified`, and `families` and `signals` are
//! empty.
//!
//! Nothing here executes or evaluates anything from the file.

mod text;
mod text_file;

use super::{Extraction, Failure, Options, readable_status};
use crate::report::{
    Declarations, FileReport, Metrics, ModuleReport, Orientation, ProcedureReport, Status,
    VbaProject,
};
use crate::rules::{ModuleType, RuleSet, StructuralPredicate, TraitRule};
use crate::vba::{
    Declaration, DeclarationKind, LogicalKind, LogicalLine, OptionKind, ParsedModule, Procedure,
    SourceLines, VbaError, parse, split_lines,
};

pub use self::text::analyze_modules;
pub(super) use self::text_file::{analyze_text, is_text_module};

/// The family of a procedure that no rule has scored: the rule engine (M3)
/// classifies the procedures, so until it runs none is.
const UNCLASSIFIED: &str = "unclassified";

/// The most lines that the modules of one file may total, once read.
/// [`split_lines`] bounds one module, and the sources of a project may total
/// 32 MiB, which a module of one-character lines turns into far more lines than
/// the time and the report of a file should take. The modules past this budget
/// are not read.
const MAX_FILE_LINES: u32 = 1_000_000;

/// The most procedures that the modules of one file may report in all. A
/// procedure can take a single line, so the line budget alone would let a
/// hostile file turn a million lines into a million procedure reports. Real
/// projects are not expected to come near this. A module that would take the
/// file past it is not reported.
const MAX_FILE_PROCEDURES: u32 = 50_000;

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
    /// The procedures that the modules may still report.
    procedures_left: u32,
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
            procedures_left: MAX_FILE_PROCEDURES,
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
            Err(error) => self.fail(&module.name, error),
        }
    }

    /// Keeps, for the file, why the module called `name` was not read.
    fn fail(&mut self, name: &str, error: VbaError) {
        self.failures.push(Failure::from(error).in_module(name));
    }

    /// Reads the next module. One that crosses the budget of lines is read
    /// whole, and the ones after it are not read, but for the empty ones. One
    /// that has more procedures than the file may still report is not kept.
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
        let procedures = count(analysis.module.procedures.len());
        if procedures > self.procedures_left {
            return Err(VbaError::new(
                Status::TooLarge,
                "vba.too_many_procedures",
                format!("the modules of the file have more than {MAX_FILE_PROCEDURES} procedures"),
            ));
        }
        self.procedures_left = self.procedures_left.saturating_sub(procedures);
        Ok(analysis)
    }

    /// Puts what `analysis` shows of a module into its report: the sizes of the
    /// module and its procedures with their spans, and its declarations.
    fn fill(&mut self, module: &mut ModuleReport, analysis: &Analysis) {
        let lines = &analysis.lines;
        module.procedures = (analysis.module.procedures.iter())
            .map(|procedure| procedure_report(procedure, lines, &self.recovery_traits))
            .collect();
        let declarations = declarations_report(&analysis.module.declarations);
        module.metrics = Metrics {
            loc: lines.physical_lines,
            logical_loc: logical_lines(&lines.logical),
            procedures: Some(count(module.procedures.len())),
            declare_count: declarations.declare_count,
            ..Metrics::default()
        };
        module.declarations = Some(declarations);
        self.recovered |= analysis.module.is_recovered();
    }

    /// Completes the report of a file once its modules are read: the metrics of
    /// the file sum those of its modules, the errors of the front end join those
    /// of the extraction, and the status follows from the procedures.
    ///
    /// A file that extraction could not read keeps the status of its first
    /// failure. Otherwise the first failure of the front end gives the status,
    /// and without one the file is `ok` if a module has a procedure or is
    /// suspected of having lost its source, or the file has macro sheets or a
    /// workbook whose encryption hides them, and `no_macros` if not.
    ///
    /// The signals of the file are not touched: extraction made them from the
    /// container indicators.
    fn seal(self, file: &mut FileReport) {
        if let Some(project) = &file.vba_project {
            file.metrics = Some(file_metrics(project));
        }
        if matches!(file.status, Status::Ok | Status::NoMacros) {
            file.status = match self.failures.first() {
                Some(first) => first.status,
                None => {
                    let modules = (file.vba_project.iter()).flat_map(|project| &project.modules);
                    let macros = modules.into_iter().any(has_macros);
                    readable_status(macros, file.xlm.as_ref(), file.workbook_encrypted)
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
    let Extraction {
        mut file, sources, ..
    } = extraction;
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

/// Whether a module has macros: a procedure, or a source that is suspected to
/// be lost with the procedures it had (they are compiled, out of sight).
fn has_macros(module: &ModuleReport) -> bool {
    !module.procedures.is_empty() || module.source_missing_suspected == Some(true)
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

/// What the declarations outside the procedures of a module say: how many
/// `Declare` statements and `WithEvents` variables, and whether `Option
/// Explicit` and `Option VBASupport` are set. A routine that `#If VBA7` declares
/// in two forms counts twice, as its statements do. The conditional
/// compilation is not evaluated.
fn declarations_report(declarations: &[Declaration]) -> Declarations {
    let (mut declares, mut with_events) = (0u32, 0u32);
    let (mut explicit, mut vba_support) = (false, false);
    for declaration in declarations {
        match &declaration.kind {
            DeclarationKind::Declare { .. } => declares = declares.saturating_add(1),
            DeclarationKind::WithEvents { .. } => with_events = with_events.saturating_add(1),
            DeclarationKind::Option(OptionKind::Explicit) => explicit = true,
            DeclarationKind::Option(OptionKind::VbaSupport) => vba_support = true,
            _ => {}
        }
    }
    Declarations {
        declare_count: Some(declares),
        with_events_count: Some(with_events),
        option_explicit: Some(explicit),
        option_vba_support: Some(vba_support),
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
    let (mut procedures, mut forms, mut declares) = (0u32, 0u32, 0u32);
    for module in &project.modules {
        metrics.loc = metrics.loc.saturating_add(module.metrics.loc);
        metrics.logical_loc = (metrics.logical_loc).saturating_add(module.metrics.logical_loc);
        procedures = procedures.saturating_add(count(module.procedures.len()));
        forms = forms.saturating_add(u32::from(module.module_type == ModuleType::Form));
        declares = declares.saturating_add(module.metrics.declare_count.unwrap_or(0));
    }
    metrics.procedures = Some(procedures);
    metrics.forms = Some(forms);
    metrics.declare_count = Some(declares);
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
    fn the_procedures_of_a_file_are_reported_within_a_budget() {
        let mut front = front();
        front.procedures_left = 3;
        let two = "Sub A()\nEnd Sub\nSub B()\nEnd Sub\n";

        assert!(front.analyse(two).is_ok());
        // One procedure is left, and a module with more is refused, whole.
        let error = front.analyse(two).err().expect("over the budget");
        assert_eq!(
            (error.status, error.code),
            (Status::TooLarge, "vba.too_many_procedures")
        );
        assert!(front.analyse("Sub C()\nEnd Sub\n").is_ok());
        assert!(front.analyse("Dim x As Long\n").is_ok());
        assert!(front.analyse("Sub D()\nEnd Sub\n").is_err());
    }

    #[test]
    fn a_module_weighs_its_physical_or_its_logical_lines() {
        let weight = |text| Analysis::of(text).expect("a short module").weight();

        assert_eq!(weight("a\nb\nc"), 3);
        assert_eq!(weight("a: b: c: d"), 4);
        assert_eq!(weight(""), 0);
    }
}
