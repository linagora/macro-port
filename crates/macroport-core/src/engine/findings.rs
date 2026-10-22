// SPDX-License-Identifier: Apache-2.0
//! The whole rule engine on one file (SPEC M3).
//!
//! [`run`] applies the rules to a file whose modules have been parsed and
//! measured. It takes the [`ProjectInput`] that the output of the parser is
//! converted into, and runs the three engines in the order of their
//! dependencies, which is the order of the SPEC:
//!
//! 1. the families score every procedure and the file ([`families::classify`]);
//! 2. the security rules read those families, for the signals that
//!    `requires_family` (see [`security::evaluate`]);
//! 3. the complexity rules read the metrics and give the level of the file and
//!    the traits of its procedures and of the file ([`complexity`]).
//!
//! The result is [`Findings`]: the fields of the report that the rules decide,
//! as plain values and in the shape the report wants them, so that the caller
//! copies them over. What is left to the caller is what the engine does not
//! know: names, kinds, lines, metrics, status, and whether a field that is
//! optional in the report is written when it is empty.
//!
//! [`complexity`]: super::complexity

use super::complexity::{complexity_level, file_traits, procedure_traits};
use super::eval::Evaluator;
use super::families;
use super::input::ProjectInput;
use super::security;
use crate::report::{FamilyScore, Level, Metrics, Signal};
use crate::rules::{RuleSet, Severity};

/// The metrics of a project that the complexity rules read, measured by the
/// caller (SPEC M2 item 7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMetrics {
    /// `procedures[m][p]` are the metrics of the procedure `p` of the module
    /// `m` of the [`ProjectInput`]. A procedure with no entry is read as having
    /// measured nothing: all its metrics count 0. An entry with no procedure is
    /// ignored.
    pub procedures: Vec<Vec<Metrics>>,
    /// The metrics of the file, aggregated by the caller as SPEC M2 item 7 says:
    /// `cyclomatic` is the highest of the procedures and `cyclomatic_total` the
    /// sum. `run` derives nothing from the procedures: a metric left out counts
    /// 0, so the criteria on it never hold.
    pub file: Metrics,
}

/// The metrics of a procedure that nobody measured.
const UNMEASURED: Metrics = Metrics {
    loc: 0,
    logical_loc: 0,
    procedures: None,
    cyclomatic: None,
    cyclomatic_total: None,
    max_nesting: None,
    parameters: None,
    goto_count: None,
    forms: None,
    declare_count: None,
    external_dependencies: None,
};

impl ProjectMetrics {
    /// The metrics of the procedure `procedure` of the module `module`, or those
    /// of a procedure that nobody measured when the caller gave none.
    fn of_procedure(&self, module: usize, procedure: usize) -> &Metrics {
        let of_module = self.procedures.get(module);
        of_module
            .and_then(|metrics| metrics.get(procedure))
            .unwrap_or(&UNMEASURED)
    }
}

/// What the rules decide about a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Findings {
    /// One entry for each module of the project, in the same order.
    pub modules: Vec<ModuleFindings>,
    /// The file.
    pub file: FileFindings,
}

/// What the rules decide about a module.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleFindings {
    /// The signals of module scope: `ModuleReport::signals`.
    pub signals: Vec<Signal>,
    /// One entry for each procedure of the module, in the same order.
    pub procedures: Vec<ProcedureFindings>,
}

/// What the rules decide about a procedure: the fields of `ProcedureReport`
/// that bear these names.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureFindings {
    /// The family that the procedure is about, or `unclassified`.
    pub primary_family: String,
    /// The other families that score close to it, best first.
    pub secondary_families: Vec<String>,
    /// The scores of the families, best first, with their evidence.
    pub families: Vec<FamilyScore>,
    /// The security signals of the procedure.
    pub signals: Vec<Signal>,
    /// The ids of all the traits of the procedure, in the order of the rules,
    /// those that the parser's predicates give (`compile_error_suspected`)
    /// included: the caller writes these instead of adding them to the ones it
    /// has marked.
    pub traits: Vec<String>,
}

/// What the rules decide about a file: the fields of `FileReport` that bear
/// these names.
#[derive(Debug, Clone, PartialEq)]
pub struct FileFindings {
    /// The family that the file is about, or `unclassified`.
    pub primary_family: String,
    /// The sums of the scores of the procedures and of the code outside them,
    /// best first, with no evidence (see [`families::classify`]).
    pub families: Vec<FamilyScore>,
    /// The signals of the file: the container indicators, those of project
    /// scope and the chains.
    pub signals: Vec<Signal>,
    /// The highest severity among all the signals of the file, those of its
    /// modules and procedures and the container indicators included.
    pub max_severity: Option<Severity>,
    /// The complexity level of the file.
    pub complexity_level: Level,
    /// The ids of all the traits of the file, in the order of the rules, as for
    /// a procedure. A file with no code has those that its metrics give,
    /// `trivial` among them: whether to report them is for the caller to
    /// decide.
    pub traits: Vec<String>,
}

/// Runs the rules on a project: its families, then its security signals, then
/// its complexity.
///
/// `container_indicators` are the ids that extraction raised, such as
/// `sec.vbaproject-in-xlsx`. Pure and deterministic.
pub fn run(
    project: &ProjectInput,
    rules: &RuleSet,
    metrics: &ProjectMetrics,
    container_indicators: &[&str],
) -> Findings {
    let scored = families::classify(project, &rules.families);
    let held: Vec<Vec<security::ProcedureFamilies>> = scored
        .modules
        .iter()
        .map(|module| module.iter().map(for_security).collect())
        .collect();
    let raised = security::evaluate(project, &rules.security, &held, container_indicators);
    let max_severity = raised.max_severity();

    // The complexity rules have an evaluator of their own: `classify` keeps the
    // one it builds for the families. It reads a ProgID `*` differently, which
    // does not matter here, as no trait matches a ProgID.
    let evaluator = Evaluator::new(project);
    let modules = (scored.modules.into_iter().zip(raised.modules))
        .enumerate()
        .map(|(module_at, (module_scores, module_signals))| {
            let found = module_scores.into_iter().zip(module_signals.procedures);
            let procedures = found.enumerate().map(|(procedure_at, (scores, signals))| {
                let measured = metrics.of_procedure(module_at, procedure_at);
                ProcedureFindings {
                    primary_family: scores.primary_family,
                    secondary_families: scores.secondary_families,
                    families: scores.families,
                    signals,
                    traits: procedure_traits(
                        &rules.complexity,
                        &evaluator,
                        module_at,
                        procedure_at,
                        measured,
                    ),
                }
            });
            ModuleFindings {
                signals: module_signals.signals,
                procedures: procedures.collect(),
            }
        })
        .collect();
    let file = FileFindings {
        primary_family: scored.file.primary_family,
        families: scored.file.families,
        signals: raised.file,
        max_severity,
        complexity_level: complexity_level(&rules.complexity, &metrics.file),
        traits: file_traits(&rules.complexity, &evaluator, &metrics.file),
    };
    Findings { modules, file }
}

/// The families of a procedure as `requires_family` reads them.
fn for_security(scores: &families::ProcedureFamilies) -> security::ProcedureFamilies {
    security::ProcedureFamilies {
        primary: scores.primary_family.clone(),
        secondary: scores.secondary_families.clone(),
    }
}

#[cfg(test)]
mod tests;
