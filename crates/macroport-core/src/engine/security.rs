// SPDX-License-Identifier: Apache-2.0
//! The security signals of a file (SPEC M3): where the signals of
//! `security.yaml` match, and the container indicators that extraction
//! raised.
//!
//! [`evaluate`] runs the matchers over a [`ProjectInput`] and returns the
//! signals placed where the report carries them, as [`FileSignals`]: those of
//! a procedure go to `ProcedureReport::signals`, those of a module to
//! `ModuleReport::signals`, and those of the file (container indicators and
//! project scope) to `FileReport::signals`. The `max_severity` of the report
//! is [`FileSignals::max_severity`].
//!
//! Two inputs come from outside the engine. The families of the procedures,
//! which `requires_family` reads and the family scoring decides, come as
//! [`ProcedureFamilies`], built from the `primary_family` and
//! `secondary_families` of each procedure, so that this code does not depend
//! on the scoring. The container indicators are the ids that the extraction
//! layer raised, such as `sec.vbaproject-in-xlsx`, and the rules give their
//! severity.

use std::collections::BTreeMap;

use super::eval::Evaluator;
use super::hit::Hit;
use super::input::{ModuleInput, ProcedureInput, ProjectInput};
use crate::report::{Evidence, Signal};
use crate::rules::{ContainerIndicator, Scope, Security, SecuritySignal, Severity};

/// The most evidence items that one signal cites. A procedure that calls
/// `Shell` a thousand times needs no thousand lines to be reviewed, and a
/// hostile file could otherwise make a report as large as itself.
pub const MAX_EVIDENCE_PER_SIGNAL: usize = 10;

/// The families of a procedure that `requires_family` reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcedureFamilies {
    /// The `primary_family` of the procedure.
    pub primary: String,
    /// Its `secondary_families`.
    pub secondary: Vec<String>,
}

impl ProcedureFamilies {
    /// Whether `family` is the primary or a secondary family.
    fn includes(&self, family: &str) -> bool {
        self.primary == family || self.secondary.iter().any(|secondary| secondary == family)
    }
}

/// The signals of a file, placed where the report carries them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSignals {
    /// One entry for each module of the project, in the same order.
    pub modules: Vec<ModuleSignals>,
    /// The signals of the file: the container indicators, then the signals of
    /// project scope. The lines of the evidence of a project-scope signal are
    /// those of its `module`, so there is one signal for each module that has
    /// hits.
    pub file: Vec<Signal>,
}

impl FileSignals {
    /// Every signal of the file, wherever the report places it and in the
    /// order of the report: for each module the signals of its procedures and
    /// then its own, and then the signals of the file.
    pub fn signals(&self) -> impl Iterator<Item = &Signal> {
        let in_modules = self.modules.iter().flat_map(|module| {
            let procedures = module.procedures.iter().flatten();
            procedures.chain(&module.signals)
        });
        in_modules.chain(&self.file)
    }

    /// The highest severity among all the signals, the container indicators
    /// included, `None` when there is none.
    pub fn max_severity(&self) -> Option<Severity> {
        self.signals().map(|signal| signal.severity).max()
    }
}

/// The signals of a module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSignals {
    /// The signals of module scope.
    pub signals: Vec<Signal>,
    /// The signals of each procedure of the module, in the same order.
    pub procedures: Vec<Vec<Signal>>,
}

/// Evaluates the security rules over a project.
///
/// - `families` gives the families of each procedure, by module and then by
///   procedure, as `project` lists them. A procedure with no entry has none,
///   so no signal that `requires_family` is raised for it.
/// - `container_indicators` are the ids that extraction raised. An id that
///   `rules` does not list is ignored, which is how a rules file switches an
///   indicator off.
///
/// A signal is raised where it has a hit: in each procedure, in each module or
/// for the whole project, according to its scope. Its evidence cites each line
/// once, in the order of the source, up to [`MAX_EVIDENCE_PER_SIGNAL`] lines.
/// The signals of a procedure follow the order of the rules.
///
/// A signal with a `requires_family` is raised in a procedure that has this
/// family as its primary or a secondary one. A module has a family when one of
/// its procedures does, and a signal of module or project scope is raised for
/// the modules that have it. At project scope, a hit that belongs to no module
/// (a reference) needs a procedure of the project to have the family.
pub fn evaluate(
    project: &ProjectInput,
    rules: &Security,
    families: &[Vec<ProcedureFamilies>],
    container_indicators: &[&str],
) -> FileSignals {
    let run = Run {
        project,
        evaluator: Evaluator::new(project),
        rules,
        families,
    };
    let mut modules = Vec::with_capacity(project.modules.len());
    for (module_at, module) in project.modules.iter().enumerate() {
        let procedures = module.procedures.iter().enumerate();
        modules.push(ModuleSignals {
            signals: run.module_signals(module_at, module),
            procedures: procedures
                .map(|(procedure_at, procedure)| {
                    run.procedure_signals(module_at, module, procedure_at, procedure)
                })
                .collect(),
        });
    }
    let mut file = indicator_signals(&rules.container_indicators, container_indicators);
    file.extend(run.project_signals());
    FileSignals { modules, file }
}

/// What the evaluation of a project reads.
struct Run<'a> {
    project: &'a ProjectInput,
    evaluator: Evaluator<'a>,
    rules: &'a Security,
    families: &'a [Vec<ProcedureFamilies>],
}

/// What a signal is evaluated over, which tells whose families its
/// `requires_family` reads.
#[derive(Debug, Clone, Copy)]
enum Subject {
    /// A procedure, by the indexes of its module and of itself in the module.
    Procedure { module: usize, procedure: usize },
    /// A module, by its index.
    Module(usize),
    /// The whole project.
    Project,
}

impl Run<'_> {
    /// The signals of a procedure, given the indexes of its module in the
    /// project and of the procedure in its module.
    fn procedure_signals(
        &self,
        module_at: usize,
        module: &ModuleInput,
        procedure_at: usize,
        procedure: &ProcedureInput,
    ) -> Vec<Signal> {
        let subject = Subject::Procedure {
            module: module_at,
            procedure: procedure_at,
        };
        self.rules_of(Scope::Procedure, subject)
            .filter_map(|rule| {
                let hits = self.evaluator.procedure_hits(
                    module_at,
                    procedure_at,
                    &rule.matcher,
                    rule.text,
                );
                raise(rule, hits, Some(&module.name), Some(&procedure.name))
            })
            .collect()
    }

    /// The signals of the module `at`, read over its declarations and all its
    /// procedures.
    fn module_signals(&self, at: usize, module: &ModuleInput) -> Vec<Signal> {
        self.rules_of(Scope::Module, Subject::Module(at))
            .filter_map(|rule| {
                let hits = self.evaluator.module_hits(at, &rule.matcher, rule.text);
                raise(rule, hits, Some(&module.name), None)
            })
            .collect()
    }

    /// The signals of project scope. The hits are grouped by module, because a
    /// line only means something in its module: a signal for each module that
    /// has hits, in the order of the modules. The hits that have no line, a
    /// reference of the project, make a signal of their own, with no module.
    fn project_signals(&self) -> Vec<Signal> {
        let mut signals = Vec::new();
        let rules = self.rules.signals.iter();
        for rule in rules.filter(|rule| rule.scope == Scope::Project) {
            let mut by_module: BTreeMap<Option<usize>, Vec<Hit<'_>>> = BTreeMap::new();
            for hit in self.evaluator.project_hits(&rule.matcher, rule.text) {
                let module = hit.place.map(|place| place.module);
                by_module.entry(module).or_default().push(hit);
            }
            for (module, hits) in by_module {
                if !self.applies(rule, module.map_or(Subject::Project, Subject::Module)) {
                    continue;
                }
                let found = module.and_then(|at| self.project.modules.get(at));
                let name = found.map(|module| module.name.as_str());
                signals.extend(raise(rule, hits, name, None));
            }
        }
        signals
    }

    /// The rules of this scope that apply to the subject, in the order of the
    /// rules file.
    fn rules_of(&self, scope: Scope, subject: Subject) -> impl Iterator<Item = &SecuritySignal> {
        let in_scope = self
            .rules
            .signals
            .iter()
            .filter(move |rule| rule.scope == scope);
        in_scope.filter(move |rule| self.applies(rule, subject))
    }

    /// Whether the rule applies to the subject: it requires no family, or the
    /// subject has it.
    fn applies(&self, rule: &SecuritySignal, subject: Subject) -> bool {
        let required = rule.requires_family.as_deref();
        required.is_none_or(|family| self.has_family(subject, family))
    }

    /// Whether the subject has this family: a procedure as its primary or a
    /// secondary one, a module or the project through one of its procedures.
    fn has_family(&self, subject: Subject, family: &str) -> bool {
        let of_module = |module: usize| self.families.get(module).map_or(&[][..], Vec::as_slice);
        match subject {
            Subject::Procedure { module, procedure } => of_module(module)
                .get(procedure)
                .is_some_and(|held| held.includes(family)),
            Subject::Module(module) => of_module(module).iter().any(|held| held.includes(family)),
            Subject::Project => (self.families.iter().flatten()).any(|held| held.includes(family)),
        }
    }
}

/// The signal of `rule` for these `hits`, none when there is no hit. The hits
/// come sorted by place, so those on one line are next to each other.
fn raise(
    rule: &SecuritySignal,
    mut hits: Vec<Hit<'_>>,
    module: Option<&str>,
    procedure: Option<&str>,
) -> Option<Signal> {
    if hits.is_empty() {
        return None;
    }
    hits.dedup_by_key(|hit| hit.place);
    let evidence: Vec<Evidence> = hits
        .iter()
        .filter_map(|hit| hit.evidence(&rule.id))
        .take(MAX_EVIDENCE_PER_SIGNAL)
        .collect();
    Some(Signal {
        signal_id: rule.id.clone(),
        severity: rule.severity,
        module: module.map(str::to_owned),
        procedure: procedure.map(str::to_owned),
        evidence: (!evidence.is_empty()).then_some(evidence),
    })
}

/// The signals of the container indicators among `raised`, in the order of
/// the rules, once each. They have no module and no evidence.
fn indicator_signals(known: &[ContainerIndicator], raised: &[&str]) -> Vec<Signal> {
    known
        .iter()
        .filter(|indicator| raised.contains(&indicator.id.as_str()))
        .map(|indicator| Signal {
            signal_id: indicator.id.clone(),
            severity: indicator.severity,
            module: None,
            procedure: None,
            evidence: None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
