// SPDX-License-Identifier: Apache-2.0
//! The modules of a file, from their source to their report (SPEC M2).
//!
//! The front end reads each module on its own, from its text alone. [`Analysis`]
//! holds what it makes of one: the lines, the procedures that the parser finds
//! in them, and what measuring each procedure shows. [`Front`] puts that into
//! the report of the module, and completes the report of the file once every
//! module is read. [`finish`] runs it on the modules of an extraction, those
//! that were found in a workbook. The text modules have two doors of their own:
//! [`analyze_modules`] (the `text` submodule) wraps modules that the caller has
//! as text in a file, and `analyze_text` (the `text_file` submodule) reads a
//! `.bas`, `.cls` or `.frm` file as the one module of a file.
//!
//! The file is `ok` when a module has a procedure or is suspected of having lost
//! its source, when it has Excel 4.0 macro sheets or when the encryption of the
//! workbook hides its sheets, and `no_macros` when none of these holds, the
//! project being described all the same. A module that cannot be read keeps the
//! report of a module that no stage measured, and its failure gives the file its
//! status.
//!
//! Only what the parser, the metrics and the statements show is filled so far:
//! the spans of the procedures and their metrics (sizes, cyclomatic complexity,
//! nesting, parameters and `GoTo`s), the APIs that they use (the `apis`
//! submodule, ADR-0011), the procedures that they call (the call graph,
//! ADR-0012), the metrics of the modules and of the file (their sizes, their
//! counts, what their procedures add up to, and the external dependencies of
//! the file: the ProgIDs of its statements, its references and its `Declare`
//! statements), the declarations of the modules (and the count of their
//! `Declare` statements, a metric), and the trait that the recoveries of the
//! parser, the blocks that do not match and the statements nested too deep to
//! be read give. The other fields are left out until the stages that measure
//! them run, since a missing value is not a zero: the external dependencies of
//! a procedure are such a field, and so are its entry point, its event handler
//! and whether nothing uses it, which wait for the rule engine to tell which
//! procedures the host starts. The exceptions are three fields of a procedure
//! that the schema requires and that only the rule engine can decide: until it
//! runs, `primary_family` is `unclassified`, and `families` and `signals` are
//! empty.
//!
//! The statements of a module are read once, in the `statements` submodule,
//! which hands the facts of each to the stages that want them: the ProgIDs of
//! the file, the lists of APIs of the procedures, the call graph and the count
//! of the statements nested too deep. Such a statement, one that is nested too
//! deep to be read in full, is no failure: the module is read, the status of
//! the file does not follow, and the file lists one error for the module
//! (`vba.statement.too_deep`). Nor is a list of APIs that passes a bound: the
//! procedure has no `apis`, and the file lists one error for the module that
//! holds such procedures (`vba.apis.left_out`).
//!
//! The call graph knows a procedure by its name in the whole project, so it
//! answers only once every module is read, and only if every module was. One
//! that has no source, that a budget refuses or whose source is suspected to be
//! lost leaves it unable to say what any procedure calls, and then the `calls`
//! of all the procedures of the file are left out (ADR-0012).
//!
//! Nothing here executes or evaluates anything from the file.

mod apis;
mod statements;
mod text;
mod text_file;

use self::apis::{FileApis, MAX_API_MIB, MAX_APIS};
use self::statements::{ExternalProgids, Statements, TooDeep};
use super::{Extraction, Failure, Options, about_module, readable_status};
use crate::report::{
    ApiUse, Declarations, FileError, FileReport, Metrics, ModuleReport, Orientation,
    ProcedureReport, Status,
};
use crate::rules::{RuleSet, StructuralPredicate, TraitRule};
use crate::vba::{
    CallGraph, Declaration, DeclarationKind, MAX_NESTING, ModuleId, OptionKind, ParsedModule,
    Procedure, ProcedureLinks, ProcedureMeasure, SourceLines, VbaError, file_metrics,
    measure_procedure, module_metrics, parse, split_lines,
};

pub use self::text::analyze_modules;
pub(super) use self::text_file::{analyze_text, is_text_module};

/// The family of a procedure that no rule has scored: the rule engine (M3)
/// classifies the procedures, so until it runs none is.
const UNCLASSIFIED: &str = "unclassified";

/// The code of the error that tells of the statements of a module that are
/// nested too deep to be read in full. It is no failure.
const TOO_DEEP: &str = "vba.statement.too_deep";

/// The code of the error that tells of the procedures of a module whose list of
/// APIs is left out, since it passed a bound. It is no failure.
const APIS_LEFT_OUT: &str = "vba.apis.left_out";

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
    /// What measuring each procedure shows: one for each of the procedures of
    /// `module`, in the same order.
    pub(super) measures: Vec<ProcedureMeasure>,
}

impl Analysis {
    /// Reads `text`. Fails only on a text of too many lines.
    fn of(text: &str) -> Result<Self, VbaError> {
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

    /// Whether the parser or the metrics show that the procedure at `index` did
    /// not parse cleanly (the structural predicate `parse_recovered`, SPEC M2
    /// item 6): the parser stepped over something in it, or its blocks do not
    /// match. A statement nested too deep to be read is a third cause, which
    /// only the statements show ([`Statements::is_deep`]): [`Front::fill`] puts
    /// the three together.
    pub(super) fn procedure_recovered(&self, index: usize) -> bool {
        let parsed = (self.module.procedures.get(index)).is_some_and(Procedure::is_recovered);
        parsed || (self.measures.get(index)).is_some_and(|measure| measure.unbalanced_blocks)
    }

    /// Whether the parser or the metrics show that the module did not parse
    /// cleanly: in a procedure, or in the code outside the procedures. As for a
    /// procedure, a statement nested too deep to be read is a third cause that
    /// they do not show.
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
struct Front {
    /// The lines that the modules may still take.
    budget: u32,
    /// The procedures that the modules may still report.
    procedures_left: u32,
    /// The ids of the traits that the rules give to the predicate
    /// `parse_recovered`. The parser, the metrics and the statements decide
    /// that one, so the procedures and the file that have it can be marked
    /// before the rule engine runs.
    recovery_traits: Vec<String>,
    /// Whether `parse_recovered` holds for a module read so far.
    recovered: bool,
    /// The ProgIDs that the statements of the modules read so far pass to
    /// `CreateObject` and `GetObject`, those that count as external
    /// dependencies.
    external_progids: ExternalProgids,
    /// The functions that the lists of APIs name, and the room that the modules
    /// read so far have left them.
    file_apis: FileApis,
    /// The calls of the procedures of the modules read so far. It has a place
    /// for each module of the file, read or not, in the order of the report.
    graph: CallGraph,
    /// What the front end tells of the modules that it read: errors of the file
    /// that are no failure.
    warnings: Vec<FileError>,
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
            external_progids: ExternalProgids::default(),
            file_apis: FileApis::new(rules),
            graph: CallGraph::new(),
            warnings: Vec::new(),
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

    /// Keeps, for the file, why the module called `name` was not read, and
    /// gives the module its place in the call graph as one that was not read
    /// ([`Front::skip`]).
    fn fail(&mut self, name: &str, error: VbaError) {
        self.skip();
        self.failures.push(Failure::from(error).in_module(name));
    }

    /// Gives the call graph the place of a module that was not read, so that it
    /// has a place for each module of the file, in the order of the report. Such
    /// a module may declare and call any procedure, so the graph then says
    /// nothing of what the procedures call (ADR-0012).
    fn skip(&mut self) -> ModuleId {
        self.graph.skip_module()
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
    /// with their spans, metrics and APIs, the metrics of the module, and its
    /// declarations. What its statements show goes to the file: the ProgIDs,
    /// the calls (the graph tells them in [`Front::seal`], once every module is
    /// read), a warning for the statements that nest too deep to be read, and
    /// one for the procedures whose list of APIs passed a bound.
    ///
    /// Three things show that code did not parse cleanly (the predicate
    /// `parse_recovered`, SPEC M2 item 6): the parser stepped over something,
    /// the blocks of a procedure do not match, and a statement nests too deep to
    /// be read. A procedure that has one of them gets the traits of the
    /// predicate, and so does the file once a module has one, in a procedure or
    /// outside the procedures.
    fn fill(&mut self, module: &mut ModuleReport, analysis: &Analysis) {
        let id = self.place_in_graph(module, analysis);
        let mut statements = Statements::read(
            analysis,
            &mut self.external_progids,
            &mut self.file_apis,
            &mut self.graph,
            id,
        );
        let found = (analysis.module.procedures.iter()).zip(&analysis.measures);
        module.procedures = (found.enumerate())
            .map(|(index, (procedure, measure))| {
                let recovered = analysis.procedure_recovered(index) || statements.is_deep(index);
                let traits = self.recovery_traits_if(recovered);
                procedure_report(procedure, measure, traits, statements.take_apis(index))
            })
            .collect();
        let declarations = declarations_report(&analysis.module.declarations);
        let procedure_metrics = analysis.measures.iter().map(|measure| &measure.metrics);
        module.metrics = Metrics {
            declare_count: declarations.declare_count,
            ..module_metrics(&analysis.lines, procedure_metrics)
        };
        module.declarations = Some(declarations);
        self.recovered |= analysis.is_recovered() || statements.too_deep.is_some();
        if let Some(deep) = statements.too_deep {
            self.warnings.push(too_deep_error(&module.name, deep));
        }
        if let Some(left_out) = LeftOut::of(&module.procedures) {
            self.warnings
                .push(apis_left_out_error(&module.name, left_out));
        }
    }

    /// Gives `module` its place in the call graph. One whose source is
    /// suspected to be lost has procedures that are compiled, out of sight, and
    /// the graph cannot tell what they call: it takes the module for one that
    /// was not read. No procedure is given as started by the host: the rule
    /// engine (M3) decides that, and the calls do not depend on it.
    fn place_in_graph(&mut self, module: &ModuleReport, analysis: &Analysis) -> ModuleId {
        if module.source_missing_suspected == Some(true) {
            return self.skip();
        }
        (self.graph).add_module(module.module_type, &analysis.module.procedures, &[])
    }

    /// The traits of a procedure or of a file that `recovered` says did not
    /// parse cleanly: those that the rules tie to the predicate
    /// `parse_recovered`. `None` if it parsed cleanly, or if no rule has that
    /// predicate.
    fn recovery_traits_if(&self, recovered: bool) -> Option<Vec<String>> {
        (recovered && !self.recovery_traits.is_empty()).then(|| self.recovery_traits.clone())
    }

    /// Completes the report of a file once its modules are read: the metrics of
    /// the file follow from those of its modules and of their procedures and
    /// from the ProgIDs of their statements, the calls of the procedures follow
    /// from the call graph, the errors of the front end join those of the
    /// extraction, and the status follows from the procedures.
    ///
    /// A file that extraction could not read keeps the status of its first
    /// failure. Otherwise the first failure of the front end gives the status,
    /// and without one the file is `ok` if a module has a procedure or is
    /// suspected of having lost its source, or the file has macro sheets or a
    /// workbook whose encryption hides them, and `no_macros` if not. The
    /// warnings on the modules that were read do not change the status.
    ///
    /// The signals of the file are not touched: extraction made them from the
    /// container indicators.
    fn seal(self, file: &mut FileReport) {
        if let Some(project) = &file.vba_project {
            file.metrics = Some(file_metrics(project, self.external_progids.iter()));
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
        if let Some(traits) = self.recovery_traits_if(self.recovered) {
            file.traits = Some(traits);
        }
        if let Some(project) = file.vba_project.as_mut() {
            put_calls(&mut project.modules, self.graph.resolve());
        }
        let errors = file.errors.get_or_insert_with(Vec::new);
        errors.extend(self.failures.into_iter().map(|failure| failure.error));
        errors.extend(self.warnings);
    }
}

/// Puts the calls that the call graph found into the procedures of `modules`.
/// `links` has the modules of the file in their order, and for each of them its
/// procedures in theirs. A procedure that the graph has no answer for keeps its
/// `calls` as `None`.
fn put_calls(modules: &mut [ModuleReport], links: Vec<Vec<ProcedureLinks>>) {
    for (module, links) in modules.iter_mut().zip(links) {
        for (procedure, links) in module.procedures.iter_mut().zip(links) {
            procedure.calls = links.calls;
        }
    }
}

/// The error of the file that tells of the statements of the module `module`
/// that are nested too deep to be read in full. The reader reads the rest of
/// such a statement flat, and the message says what that loses.
fn too_deep_error(module: &str, deep: TooDeep) -> FileError {
    let TooDeep { count, first_line } = deep;
    let nest = if count == 1 {
        "statement nests"
    } else {
        "statements nest"
    };
    let message = format!(
        "{count} {nest} expressions deeper than {MAX_NESTING} levels, the first at line \
         {first_line}; the rest of such a statement is read flat: no chain, ProgID or mark of \
         obfuscation is found in it, its string literals are listed one by one with none \
         folded, and a name that stands alone counts as a call"
    );
    FileError {
        code: TOO_DEEP.to_owned(),
        message: about_module(module, &message),
    }
}

/// The procedures of a module whose list of APIs is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LeftOut {
    /// How many there are.
    count: u32,
    /// The line where the first starts.
    first_line: u32,
}

impl LeftOut {
    /// The procedures among `procedures` that have no `apis`, if there are any.
    fn of(procedures: &[ProcedureReport]) -> Option<Self> {
        let mut left_out = (procedures.iter()).filter(|procedure| procedure.apis.is_none());
        let first = left_out.next()?;
        Some(Self {
            count: count(left_out.count()).saturating_add(1),
            first_line: first.start_line,
        })
    }
}

/// The error of the file that tells of the procedures of the module `module`
/// whose list of APIs is left out, and why.
fn apis_left_out_error(module: &str, left_out: LeftOut) -> FileError {
    let LeftOut { count, first_line } = left_out;
    let (procedures, start, lists) = if count == 1 {
        ("procedure uses", "it starts", "its list is")
    } else {
        ("procedures use", "the first starts", "their lists are")
    };
    let message = format!(
        "{count} {procedures} more APIs than a report lists (at most {MAX_APIS} names for a \
         procedure and {MAX_API_MIB} MiB of names for the procedures of a file); {start} at line \
         {first_line}, and {lists} left out, since a short list would read as a whole one"
    );
    FileError {
        code: APIS_LEFT_OUT.to_owned(),
        message: about_module(module, &message),
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
            match source {
                Some(text) => front.read(module, text),
                None => {
                    front.skip();
                }
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

/// The report of a procedure, as far as its structure, its measure and its
/// statements show it, with the `traits` that the front end gives it, if any,
/// and the `apis` that it uses, unless its list passed a bound.
fn procedure_report(
    procedure: &Procedure,
    measure: &ProcedureMeasure,
    traits: Option<Vec<String>>,
    apis: Option<Vec<ApiUse>>,
) -> ProcedureReport {
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
        apis,
        primary_family: UNCLASSIFIED.to_owned(),
        secondary_families: None,
        families: Vec::new(),
        signals: Vec::new(),
        traits,
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

/// A length as the `u32` that the report holds: `u32::MAX` if it is larger.
fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::text::{text_extraction, text_module};
    use super::*;
    use crate::rules::{ModuleType, RulesSources};

    fn front() -> Front {
        let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
        Front::new(&rules)
    }

    /// A module that calls `B`, and one that declares it.
    const CALLER: &str = "Sub A()\n    B\nEnd Sub\n";
    const CALLED: &str = "Public Sub B()\nEnd Sub\n";

    /// The report of the standard module `name` as extraction gives it.
    fn standard(name: &str, text: &str) -> ModuleReport {
        text_module(name, ModuleType::Standard, text)
    }

    /// The `calls` of the procedures of each module, after `finish` read the
    /// modules of these reports and these sources.
    fn finished_calls(
        reports: Vec<ModuleReport>,
        sources: Vec<Option<String>>,
    ) -> Vec<Vec<Option<Vec<String>>>> {
        let rules = RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load");
        let extraction = text_extraction("Book.bas", 0, String::new(), reports, sources, &rules);
        let file = finish(extraction, &Options::new(&rules));
        let project = file.vba_project.expect("a VBA project");
        (project.modules.into_iter())
            .map(|module| {
                (module.procedures.into_iter())
                    .map(|procedure| procedure.calls)
                    .collect()
            })
            .collect()
    }

    /// The calls of the procedure `A` of `CALLER`, and of `B` of `CALLED`, when
    /// every module is read: `A` calls `B`.
    fn read_in_full() -> Vec<Vec<Option<Vec<String>>>> {
        vec![
            vec![Some(vec!["B".to_owned()])],
            Vec::new(),
            vec![Some(Vec::new())],
        ]
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

    #[test]
    fn the_error_for_statements_nested_too_deep_names_the_module_the_count_and_the_line() {
        let error = |count, first_line| too_deep_error("Module1", TooDeep { count, first_line });
        let past = format!("expressions deeper than {MAX_NESTING} levels");

        let one = error(1, 7);
        let two = error(2, 9);

        assert_eq!(one.code, "vba.statement.too_deep");
        let first = format!("module Module1: 1 statement nests {past}, the first at line 7;");
        assert!(one.message.starts_with(&first), "{}", one.message);
        let second = format!("module Module1: 2 statements nest {past}, the first at line 9;");
        assert!(two.message.starts_with(&second), "{}", two.message);
        assert!(
            one.message
                .ends_with("and a name that stands alone counts as a call")
        );
    }

    #[test]
    fn the_procedures_without_apis_are_counted_with_the_line_of_the_first() {
        let analysis = Analysis::of("Sub A()\nEnd Sub\nSub B()\nEnd Sub\nSub C()\nEnd Sub\n")
            .expect("a short module");
        let report = |index: usize, apis| {
            let procedure = analysis.module.procedures.get(index).expect("a procedure");
            let measure = analysis.measures.get(index).expect("a measure");
            procedure_report(procedure, measure, None, apis)
        };
        let all = [
            report(0, Some(Vec::new())),
            report(1, None),
            report(2, None),
        ];

        let expected = LeftOut {
            count: 2,
            first_line: 3,
        };
        assert_eq!(LeftOut::of(&all), Some(expected));
        assert_eq!(LeftOut::of(&all[..1]), None);
        assert_eq!(LeftOut::of(&[]), None);
    }

    #[test]
    fn the_error_for_lists_of_apis_left_out_names_the_module_the_count_the_line_and_the_bounds() {
        let error =
            |count, first_line| apis_left_out_error("Module1", LeftOut { count, first_line });
        let bounds = format!("{MAX_APIS} names for a procedure and {MAX_API_MIB} MiB of names");

        let one = error(1, 7);
        let two = error(2, 9);

        assert_eq!(one.code, "vba.apis.left_out");
        let first = format!(
            "module Module1: 1 procedure uses more APIs than a report lists (at most {bounds}"
        );
        assert!(one.message.starts_with(&first), "{}", one.message);
        assert!(
            one.message
                .contains("); it starts at line 7, and its list is left out")
        );
        let second = "module Module1: 2 procedures use more APIs than a report lists";
        assert!(two.message.starts_with(second), "{}", two.message);
        assert!(
            two.message
                .contains("); the first starts at line 9, and their lists are left out")
        );
    }

    #[test]
    fn the_call_graph_keeps_a_place_for_each_module_whether_it_was_read_or_not() {
        let mut front = front();
        let two = "Sub A()\nEnd Sub\nSub B()\nEnd Sub\n";
        let one = "Sub C()\nEnd Sub\n";
        let (mut first, mut last) = (standard("First", two), standard("Last", one));
        let refused = VbaError::new(Status::TooLarge, "vba.too_many_lines", "too many");

        front.read(&mut first, two);
        front.fail("Refused", refused);
        front.skip();
        front.read(&mut last, one);

        // The failure is the only thing kept for the file: a module that has no
        // source is no failure of the front end, and the extraction tells of it.
        assert_eq!(front.failures.len(), 1);
        let places: Vec<_> = front.graph.resolve().iter().map(Vec::len).collect();
        assert_eq!(places, [2, 0, 0, 1]);
    }

    #[test]
    fn a_module_that_the_budget_refuses_leaves_no_procedure_a_list_of_calls() {
        let mut front = front();
        front.budget = 3;
        let (mut caller, mut called) = (standard("First", CALLER), standard("Last", CALLED));

        front.read(&mut caller, CALLER);
        front.read(&mut called, CALLED);

        assert_eq!(front.failures.len(), 1);
        let links = front.graph.resolve();
        // The refused module has its place, and nothing is said of the other.
        assert_eq!(links.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0]);
        assert_eq!(links[0][0].calls, None);
    }

    #[test]
    fn a_module_without_source_leaves_the_calls_of_the_file_unknown() {
        let reports = || {
            vec![
                standard("First", CALLER),
                standard("Middle", ""),
                standard("Last", CALLED),
            ]
        };
        let sources = |middle: Option<&str>| {
            vec![
                Some(CALLER.to_owned()),
                middle.map(str::to_owned),
                Some(CALLED.to_owned()),
            ]
        };

        // With a source, empty as it is, the module in the middle changes nothing.
        assert_eq!(finished_calls(reports(), sources(Some(""))), read_in_full());
        // Without one, it may hold any procedure.
        let unknown = vec![vec![None], Vec::new(), vec![None]];
        assert_eq!(finished_calls(reports(), sources(None)), unknown);
    }

    #[test]
    fn a_module_suspected_of_having_lost_its_source_leaves_the_calls_of_the_file_unknown() {
        let reports = |suspected| {
            let mut middle = standard("Middle", "");
            middle.source_missing_suspected = Some(suspected);
            vec![standard("First", CALLER), middle, standard("Last", CALLED)]
        };
        // What is left of a stomped module is `Attribute` lines and blank ones.
        let left = "Attribute VB_Name = \"Middle\"\n\n";
        let sources = vec![
            Some(CALLER.to_owned()),
            Some(left.to_owned()),
            Some(CALLED.to_owned()),
        ];

        let seen = finished_calls(reports(false), sources.clone());
        let lost = finished_calls(reports(true), sources);

        // Its compiled procedures are out of sight, and may call and be called.
        assert_eq!(seen, read_in_full());
        assert_eq!(lost, [vec![None], Vec::new(), vec![None]]);
    }
}
