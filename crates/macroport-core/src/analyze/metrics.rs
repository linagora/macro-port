// SPDX-License-Identifier: Apache-2.0
//! The metrics of the project, as the rule engine reads them (SPEC M2 item 7).
//!
//! The lines of a procedure give its metrics ([`measure_procedure`]), and
//! [`module_metrics`] and [`file_metrics`] add them up for a module and for a
//! file. What they leave out is the `external_dependencies` of a procedure, as
//! `rules/complexity.yaml` defines it: the distinct ProgIDs it uses, and the
//! distinct `Declare`d routines it calls. That needs the ProgIDs and the calls
//! of its statements, and the routines that the other modules declare, so it is
//! measured from the input of the rule engine once every module is read
//! ([`project_metrics`]), and [`report_dependencies`] gives it to the report.
//! The count is as complete as the reading of the statements: a statement
//! nested too deep is read flat from the point where it gets too deep, so a
//! ProgID past that point is not counted, and the procedure that holds the
//! statement is flagged as one that did not parse. The file does not count
//! them again: the ProgIDs that [`file_metrics`] adds to its references and its
//! `Declare` statements are those that the walk of the `statements` submodule
//! of `modules` collects.
//!
//! [`measure_procedure`]: crate::vba::measure_procedure
//! [`module_metrics`]: crate::vba::module_metrics
//! [`file_metrics`]: crate::vba::file_metrics

use std::collections::BTreeSet;

use super::input::Measured;
use crate::engine::findings::ProjectMetrics;
use crate::engine::{NameSet, ProcedureInput, ProjectInput, comparable_key};
use crate::report::{FileReport, Metrics};
use crate::vba::count_progids;

/// A length as a metric: it saturates.
pub(super) fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The metrics of the project for the rule engine: those of each procedure,
/// completed with the dependencies that it has, and those of the file.
///
/// `file` are the metrics of the file as [`file_metrics`] gives them, the
/// dependencies of the file included, and `measured` has an entry for each
/// module of `project`, with the metrics that the lines of the procedures gave.
///
/// [`file_metrics`]: crate::vba::file_metrics
pub(super) fn project_metrics(
    project: &ProjectInput,
    measured: &[Measured],
    file: Metrics,
) -> ProjectMetrics {
    let declared: NameSet = (measured.iter())
        .flat_map(|module| &module.declared)
        .collect();
    let procedures = (project.modules.iter().zip(measured))
        .map(|(module, measured)| {
            (module.procedures.iter().zip(&measured.procedures))
                .map(|(procedure, metrics)| Metrics {
                    external_dependencies: Some(procedure_dependencies(procedure, &declared)),
                    ..metrics.clone()
                })
                .collect()
        })
        .collect();
    ProjectMetrics { procedures, file }
}

/// Writes into the report of `file` the dependencies that `metrics` counts for
/// its procedures. The report of a procedure is made when its module is read,
/// and a routine that it calls may be declared by a module read after it, so
/// the count waits for the whole project. `metrics` has an entry for each
/// module of the report, with no count for a module that was not read: its
/// report is left as it is.
pub(super) fn report_dependencies(file: &mut FileReport, metrics: &ProjectMetrics) {
    let Some(project) = file.vba_project.as_mut() else {
        return;
    };
    for (module, counts) in project.modules.iter_mut().zip(&metrics.procedures) {
        for (procedure, counted) in module.procedures.iter_mut().zip(counts) {
            procedure.metrics.external_dependencies = counted.external_dependencies;
        }
    }
}

/// The external dependencies of a procedure: the distinct ProgIDs it uses, and
/// the distinct `Declare`d routines it calls.
fn procedure_dependencies(procedure: &ProcedureInput, declared: &NameSet) -> u32 {
    let progids = count_progids(procedure.lines.iter().flat_map(|line| &line.progids));
    let called: BTreeSet<String> = (procedure.lines.iter())
        .flat_map(|line| &line.calls)
        .filter(|call| !call.after_dot && declared.contains(&call.name))
        .map(|call| comparable_key(&call.name))
        .collect();
    progids.saturating_add(count(called.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CallSite, CodeLine, ModuleInput, ParserPredicates};
    use crate::rules::ModuleType;

    fn line(progids: &[&str], calls: &[&str]) -> CodeLine {
        CodeLine {
            progids: progids.iter().map(|progid| (*progid).to_owned()).collect(),
            calls: calls
                .iter()
                .map(|name| CallSite {
                    name: (*name).to_owned(),
                    after_dot: false,
                })
                .collect(),
            ..CodeLine::default()
        }
    }

    fn procedure(lines: Vec<CodeLine>) -> ProcedureInput {
        ProcedureInput {
            name: "Run".to_owned(),
            kind: crate::report::ProcedureKind::Sub,
            start_line: 1,
            end_line: 2,
            lines,
            predicates: ParserPredicates::default(),
        }
    }

    fn module(declarations: Vec<CodeLine>, procedures: Vec<ProcedureInput>) -> ModuleInput {
        ModuleInput {
            name: "Module1".to_owned(),
            module_type: ModuleType::Standard,
            with_events: NameSet::default(),
            controls: NameSet::default(),
            declarations,
            procedures,
            parse_recovered: false,
        }
    }

    fn measured(procedures: usize, declared: &[&str]) -> Measured {
        Measured {
            read: true,
            procedures: vec![
                Metrics {
                    cyclomatic: Some(2),
                    ..Metrics::default()
                };
                procedures
            ],
            declared: declared.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn a_procedure_counts_its_distinct_progids_and_declared_routines() {
        let declared: NameSet = ["GetTickCount", "Sleep"].into_iter().collect();
        let lines = vec![
            line(&["WScript.Shell"], &["Shell"]),
            line(
                &["wscript.shell", "Scripting.Dictionary"],
                &["gettickcount"],
            ),
            line(&["VBScript.RegExp"], &["GetTickCount", "Sleep", "Other"]),
        ];

        // One ProgID, and two routines that the module declares.
        assert_eq!(procedure_dependencies(&procedure(lines), &declared), 3);
        assert_eq!(procedure_dependencies(&procedure(Vec::new()), &declared), 0);
    }

    #[test]
    fn a_call_of_a_member_is_not_a_call_of_the_declared_routine() {
        let declared: NameSet = ["Sleep"].into_iter().collect();
        let mut member = line(&[], &["Sleep"]);
        member.calls[0].after_dot = true;

        assert_eq!(
            procedure_dependencies(&procedure(vec![member]), &declared),
            0
        );
    }

    #[test]
    fn a_string_that_is_no_progid_is_no_dependency_of_a_procedure() {
        let declared = NameSet::default();
        // What `GetObject` takes besides a ProgID: a path, and a moniker.
        let lines = vec![line(&["C:\\Reports\\book.xlsx", "winmgmts:"], &[])];

        assert_eq!(procedure_dependencies(&procedure(lines), &declared), 0);
    }

    #[test]
    fn the_metrics_of_the_project_follow_its_modules_and_procedures() {
        let project = ProjectInput {
            references: Vec::new(),
            modules: vec![
                module(
                    Vec::new(),
                    vec![
                        procedure(vec![line(&[], &["Sleep"])]),
                        procedure(vec![line(&["Word.Application"], &[])]),
                    ],
                ),
                module(Vec::new(), vec![procedure(Vec::new())]),
            ],
        };
        let measured = [measured(2, &["Sleep"]), measured(1, &[])];
        let file = Metrics {
            declare_count: Some(1),
            external_dependencies: Some(2),
            ..Metrics::default()
        };

        let metrics = project_metrics(&project, &measured, file);

        let dependencies: Vec<Vec<_>> = (metrics.procedures.iter())
            .map(|module| {
                module
                    .iter()
                    .map(|m| (m.cyclomatic, m.external_dependencies))
                    .collect()
            })
            .collect();
        assert_eq!(
            dependencies,
            [
                vec![(Some(2), Some(1)), (Some(2), Some(1))],
                vec![(Some(2), Some(0))]
            ]
        );
        // The metrics of the file are those that were given.
        assert_eq!(
            (
                metrics.file.declare_count,
                metrics.file.external_dependencies
            ),
            (Some(1), Some(2))
        );
    }
}
