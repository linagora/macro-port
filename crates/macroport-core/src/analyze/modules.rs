// SPDX-License-Identifier: Apache-2.0
//! The modules of a file, from their source to their report (SPEC M2 and M3).
//!
//! The front end reads each module on its own, from its text alone. [`Analysis`]
//! holds what it makes of one: the lines, the procedures that the parser finds
//! in them, and what measuring each procedure shows. [`Front`] puts that into
//! the report of the module, keeps what the rule engine reads of it, and once
//! every module is read, runs the engine on the project and completes the report
//! of the file. [`finish`] runs it on the modules of an extraction, those that
//! were found in a workbook. The text modules have two doors of their own:
//! [`analyze_modules`] (the `text` submodule) wraps modules that the caller has
//! as text in a file, and `analyze_text` (the `text_file` submodule) reads a
//! `.bas`, `.cls` or `.frm` file as the one module of a file.
//!
//! The file is `ok` when a module has a procedure or is suspected of having lost
//! its source, when it has Excel 4.0 macro sheets or when the encryption of the
//! workbook hides its sheets, and `no_macros` when none of these holds, the
//! project being described all the same. A module that cannot be read keeps the
//! report of a module that no stage measured, and its failure gives the file its
//! status. The rule engine runs as soon as one module is read, so that what can
//! be read of a file is judged whatever else went wrong with it, and when the
//! extraction raised a container indicator, so that its signal is made.
//!
//! The ProgIDs of the statements are not read yet, nor are the entry points and
//! the call graph: what depends on them is left out, since a missing value is
//! not a zero (see [`input`](super::input)). The external dependencies of the
//! procedures and of the file are such a field: their count would fall short, so
//! the report leaves it out, and the rule engine reads what is known of it, a
//! value that the report leaves out counting as 0 for it.
//!
//! Nothing here executes or evaluates anything from the file.

mod text;
mod text_file;

use super::apply::apply;
use super::input::{Adapted, Measured, Room, adapt};
use super::metrics::{count, progids, project_metrics};
use super::{Extraction, Failure, Options, readable_status};
use crate::engine::{ModuleInput, ProjectInput, findings};
use crate::report::{
    Declarations, FileReport, Metrics, ModuleReport, Orientation, ProcedureReport, Status,
};
use crate::rules::RuleSet;
use crate::vba::{
    Declaration, DeclarationKind, OptionKind, ParsedModule, Procedure, ProcedureMeasure,
    SourceLines, VbaError, file_metrics, measure_procedure, module_metrics, parse, split_lines,
};

pub use self::text::analyze_modules;
pub(super) use self::text_file::{analyze_text, is_text_module};

/// The family of a procedure before the rule engine has scored it, and of the
/// procedures of a module that it did not run on.
const UNCLASSIFIED: &str = "unclassified";

/// The most lines that the modules of one file may total, once read.
/// [`split_lines`] bounds one module, and the sources of a project may total
/// 32 MiB, which modules of one-character lines turn into millions of lines,
/// far more than the time, the memory and the report of a file should take: the
/// rule engine keeps what it reads of every module until the last one is read,
/// some 330 bytes for each line of one character. The modules past this budget
/// are not read, and the one that crosses it is read whole, so a file holds
/// this many lines and those of one module more
/// ([`MAX_LINES`](crate::vba::MAX_LINES)), and nothing is split once the budget
/// is spent.
///
/// The lines do not bound the memory by themselves, since the statements of a
/// line may hold any number of names: the engine's input has a room of its own
/// ([`Room`]). A budget of a million lines took 481 MB, measured on modules of
/// one-character lines, in a release build on macOS, where the limit is 256 MB
/// (ACCEPTANCE A2).
const MAX_FILE_LINES: u32 = 100_000;

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
    /// What measuring each procedure shows: one for each of the procedures of
    /// `module`, in the same order.
    pub(super) measures: Vec<ProcedureMeasure>,
}

impl Analysis {
    /// Reads `text`. Fails only on a text of too many lines.
    pub(super) fn of(text: &str) -> Result<Self, VbaError> {
        let lines = split_lines(text)?;
        let module = parse(&lines);
        let measures = (module.procedures.iter())
            .map(|procedure| measure_parsed(procedure, &lines))
            .collect();
        Ok(Self {
            lines,
            module,
            measures,
        })
    }

    /// Whether the structural predicate `parse_recovered` holds for the
    /// procedure at `index` (SPEC M2 item 6): the parser stepped over something
    /// in it, or its blocks do not match.
    pub(super) fn procedure_recovered(&self, index: usize) -> bool {
        let parsed = (self.module.procedures.get(index)).is_some_and(Procedure::is_recovered);
        parsed || (self.measures.get(index)).is_some_and(|measure| measure.unbalanced_blocks)
    }

    /// Whether `parse_recovered` holds for the module as a whole: for a
    /// procedure, or for the code outside the procedures.
    pub(super) fn is_recovered(&self) -> bool {
        self.module.is_recovered()
            || (self.measures.iter()).any(|measure| measure.unbalanced_blocks)
    }

    /// How many lines reading the module took: the greater of its physical and
    /// its logical lines, since a line can hold many statements.
    fn weight(&self) -> u32 {
        count(self.lines.logical.len()).max(self.lines.physical_lines)
    }
}

/// The front end at work on the modules of one file.
struct Front<'r> {
    /// The rules that the engine applies once the modules are read.
    rules: &'r RuleSet,
    /// The lines that the modules may still take.
    budget: u32,
    /// The procedures that the modules may still report.
    procedures_left: u32,
    /// What the rule engine may still read of the modules, in bytes of input.
    room: Room,
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
            procedures_left: MAX_FILE_PROCEDURES,
            room: Room::of_a_file(),
            inputs: Vec::new(),
            measured: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Reads `text`, the source of `module`, into its report: none for a module
    /// whose source could not be extracted. A module that cannot be read keeps
    /// the report that extraction gave it, and its failure is kept for the file.
    fn read(&mut self, module: &mut ModuleReport, text: Option<&str>) {
        let analysed = text.map(|text| self.analyse(text));
        self.keep(module, analysed);
    }

    /// Keeps `module`, the next one of the file, given what [`analyse`] made of
    /// its text (none for a module with no source): its report is filled, and
    /// what the engine reads of it is kept. A module that was not read, or that
    /// does not fit in the room of the engine, is kept all the same, empty, so
    /// that the modules of the engine are those of the report, in the same
    /// order.
    ///
    /// [`analyse`]: Self::analyse
    fn keep(&mut self, module: &mut ModuleReport, analysed: Option<Result<Analysis, VbaError>>) {
        let read = analysed.map(|analysis| analysis.and_then(|read| self.fill(module, &read)));
        let adapted = match read {
            Some(Ok(adapted)) => adapted,
            Some(Err(error)) => {
                (self.failures).push(Failure::from(error).in_module(&module.name));
                Adapted::unread(&module.name, module.module_type)
            }
            None => Adapted::unread(&module.name, module.module_type),
        };
        self.inputs.push(adapted.input);
        self.measured.push(adapted.measured);
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

    /// Puts what `analysis` shows of a module into its report: its procedures
    /// with their spans and metrics, the metrics of the module, and its
    /// declarations. What the rule engine decides is left for
    /// [`seal`](Self::seal). Fails, and leaves the report alone, when what the
    /// engine reads of the module does not fit in the room that is left.
    fn fill(
        &mut self,
        module: &mut ModuleReport,
        analysis: &Analysis,
    ) -> Result<Adapted, VbaError> {
        let adapted = adapt(&module.name, module.module_type, analysis, &mut self.room)?;
        let found = (analysis.module.procedures.iter()).zip(&analysis.measures);
        module.procedures = found
            .map(|(procedure, measure)| procedure_report(procedure, measure))
            .collect();
        let declarations = declarations_report(&analysis.module.declarations);
        let procedure_metrics = analysis.measures.iter().map(|measure| &measure.metrics);
        module.metrics = Metrics {
            declare_count: declarations.declare_count,
            ..module_metrics(&analysis.lines, procedure_metrics)
        };
        module.declarations = Some(declarations);
        Ok(adapted)
    }

    /// Completes the report of a file once its modules are read: the metrics of
    /// the file follow from those of its modules and of their procedures, the
    /// rule engine decides the families, the signals, the traits and the level,
    /// the errors of the front end join those of the extraction, and the status
    /// follows from the procedures. `indicators` are the ids of the container
    /// indicators that extraction raised: the engine makes their signals.
    ///
    /// The engine runs when a module was read, whatever the status of the file:
    /// a file of which some modules could not be read is judged on the others.
    /// It also runs when an indicator was raised, for the signals of the
    /// indicators and the severity they give the file: a file where no module
    /// was read has nothing else decided.
    ///
    /// A file that extraction could not read keeps the status of its first
    /// failure. Otherwise the first failure of the front end gives the status,
    /// and without one the file is `ok` if a module has a procedure or is
    /// suspected of having lost its source, or the file has macro sheets or a
    /// workbook whose encryption hides them, and `no_macros` if not.
    fn seal(self, file: &mut FileReport, indicators: &[&str]) {
        let Self {
            rules,
            inputs,
            measured,
            failures,
            ..
        } = self;
        let project = ProjectInput {
            references: (file.vba_project.as_ref())
                .and_then(|project| project.references.clone())
                .unwrap_or_default(),
            modules: inputs,
        };
        let sizes = (file.vba_project.as_ref()).map(|vba| file_metrics(vba, progids(&project)));
        let has_project = sizes.is_some();
        let metrics = project_metrics(&project, &measured, sizes.unwrap_or_default());
        if has_project {
            file.metrics = Some(Metrics {
                // The ProgIDs that the statements pass to `CreateObject` and
                // `GetObject` are not read yet, so the count would fall short.
                // The engine reads it all the same, as a lower bound.
                external_dependencies: None,
                ..metrics.file.clone()
            });
        }
        let judged = measured.iter().any(|module| module.read);
        if judged || !indicators.is_empty() {
            let decided = findings::run(&project, rules, &metrics, indicators);
            apply(file, decided, &measured, judged);
        }
        if matches!(file.status, Status::Ok | Status::NoMacros) {
            file.status = match failures.first() {
                Some(first) => first.status,
                None => {
                    let modules = (file.vba_project.iter()).flat_map(|project| &project.modules);
                    let macros = modules.into_iter().any(has_macros);
                    readable_status(macros, file.xlm.as_ref(), file.workbook_encrypted)
                }
            };
        }
        let errors = file.errors.get_or_insert_with(Vec::new);
        errors.extend(failures.into_iter().map(|failure| failure.error));
    }
}

/// Reads the modules that extraction found, and completes the report of the
/// file.
pub(super) fn finish(extraction: Extraction, options: &Options<'_>) -> FileReport {
    let Extraction {
        mut file,
        sources,
        indicators,
    } = extraction;
    let mut front = Front::new(options.rules);
    if let Some(project) = file.vba_project.as_mut() {
        for (at, module) in project.modules.iter_mut().enumerate() {
            let text = sources.get(at).and_then(|source| source.as_deref());
            front.read(module, text);
        }
    }
    front.seal(&mut file, &indicators);
    file
}

/// Whether a module has macros: a procedure, or a source that is suspected to
/// be lost with the procedures it had (they are compiled, out of sight).
fn has_macros(module: &ModuleReport) -> bool {
    !module.procedures.is_empty() || module.source_missing_suspected == Some(true)
}

/// What measuring a procedure that the parser found shows: its metrics, and
/// whether its blocks match. `lines` are those of the module that holds it.
fn measure_parsed(procedure: &Procedure, lines: &SourceLines) -> ProcedureMeasure {
    let code = (lines.logical.get(procedure.lines.clone())).unwrap_or_default();
    measure_procedure(
        procedure.start_line,
        procedure.end_line,
        procedure.parameter_count,
        code,
    )
}

/// The report of a procedure, as far as its structure and its measure show it.
/// The fields that the rule engine decides hold what a procedure that no rule
/// has scored has, until [`apply`] writes the findings.
fn procedure_report(procedure: &Procedure, measure: &ProcedureMeasure) -> ProcedureReport {
    ProcedureReport {
        name: procedure.name.clone(),
        kind: procedure.kind,
        visibility: Some(procedure.visibility),
        is_entry_point: None,
        is_event_handler: None,
        possibly_unused: None,
        start_line: procedure.start_line,
        end_line: procedure.end_line,
        metrics: measure.metrics.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{ModuleType, RulesSources};

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
    fn the_procedures_of_a_file_are_reported_within_a_budget() {
        let rules = rules();
        let mut front = Front::new(&rules);
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
    fn the_statements_of_a_file_are_read_within_a_room() {
        let rules = rules();
        let mut front = Front::new(&rules);
        front.room = Room::of(8_000);
        let read = |front: &mut Front, name: &str, text: &str| {
            let mut module = text::text_module(name, ModuleType::Standard, text);
            front.read(&mut module, Some(text));
            module
        };
        let small = "Sub A()\n    x = 1\nEnd Sub\n";
        // A statement of five thousand members takes more than the room.
        let dense = format!("Sub B()\n    x = {}a\nEnd Sub\n", "a.".repeat(5_000));

        assert_eq!(read(&mut front, "First", small).procedures.len(), 1);
        let refused = read(&mut front, "Second", &dense);
        // The module is listed, empty, with the failure of the file, and the
        // room it did not take is left to the modules after it.
        assert!(refused.procedures.is_empty() && refused.declarations.is_none());
        let [failure] = front.failures.as_slice() else {
            panic!("one failure expected");
        };
        assert_eq!(failure.status, Status::TooLarge);
        assert_eq!(failure.error.code, "vba.too_much_code");
        assert!(failure.error.message.starts_with("module Second: "));
        assert_eq!(read(&mut front, "Third", small).procedures.len(), 1);

        let names: Vec<_> = (front.inputs.iter())
            .map(|input| (input.name.as_str(), input.procedures.len()))
            .collect();
        assert_eq!(names, [("First", 1), ("Second", 0), ("Third", 1)]);
        assert_eq!(front.measured.len(), 3);
    }

    #[test]
    fn a_module_weighs_its_physical_or_its_logical_lines() {
        let weight = |text| Analysis::of(text).expect("a short module").weight();

        assert_eq!(weight("a\nb\nc"), 3);
        assert_eq!(weight("a: b: c: d"), 4);
        assert_eq!(weight(""), 0);
    }

    #[test]
    fn an_analysis_measures_each_procedure_in_order() {
        let analysis =
            Analysis::of("Sub A()\nEnd Sub\nSub B(x)\n    If x Then\n    End If\nEnd Sub\n")
                .expect("a short module");

        let cyclomatic: Vec<_> = (analysis.measures.iter())
            .map(|measure| measure.metrics.cyclomatic)
            .collect();
        assert_eq!(analysis.measures.len(), analysis.module.procedures.len());
        assert_eq!(cyclomatic, [Some(1), Some(2)]);
    }

    #[test]
    fn an_analysis_tells_which_procedures_did_not_parse_cleanly() {
        let of = |text| Analysis::of(text).expect("a short module");

        // The blocks of the first of three procedures do not match, and the
        // parser recovers in the second, which has no end. There is no fourth.
        let both = of("Sub A()\n    If x Then\nEnd Sub\nSub B()\nSub C()\nEnd Sub\n");
        let recovered: Vec<_> = (0..4)
            .map(|index| both.procedure_recovered(index))
            .collect();
        assert_eq!(recovered, [true, true, false, false]);
        assert!(both.is_recovered());

        // The blocks alone, and the code outside the procedures alone.
        let blocks = of("Sub A()\n    Next\nEnd Sub\n");
        assert!(!blocks.module.is_recovered());
        assert!(blocks.procedure_recovered(0) && blocks.is_recovered());
        let outside = of("Sub A()\nEnd Sub\nEnd Sub\n");
        assert!(!outside.procedure_recovered(0));
        assert!(outside.is_recovered());

        let clean = of("Sub A()\n    If x Then\n    End If\nEnd Sub\n");
        assert!(!clean.procedure_recovered(0) && !clean.is_recovered());
    }
}
