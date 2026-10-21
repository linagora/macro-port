// SPDX-License-Identifier: Apache-2.0
//! The metrics of the modules and of the file (SPEC M2 item 7).
//!
//! The lines of a procedure give its metrics ([`measure_procedure`]). A module
//! and a file aggregate them: `loc` and `logical_loc` are summed, over the
//! modules for a file, `cyclomatic` is the highest of the procedures and
//! `cyclomatic_total` their sum, and `max_nesting` the deepest. `procedures`,
//! `forms`, `declare_count` and `external_dependencies` are defined in
//! `rules/complexity.yaml`. The last one needs the references of the project and
//! the ProgIDs and the calls of its statements, so it is measured from the input
//! of the rule engine, whatever the front end has put in it: a ProgID that the
//! front end does not read yet does not count.
//!
//! [`measure_procedure`]: crate::vba::measure_procedure

use std::collections::BTreeSet;

use super::input::Measured;
use crate::engine::findings::ProjectMetrics;
use crate::engine::{CodeLine, NameSet, ProcedureInput, ProjectInput, comparable_key};
use crate::report::{Metrics, VbaProject};
use crate::rules::ModuleType;
use crate::vba::{LogicalKind, LogicalLine, SourceLines};

/// The ProgIDs that every project uses and that are no dependency
/// (`rules/complexity.yaml`).
const COMMON_PROGIDS: [&str; 2] = ["Scripting.Dictionary", "VBScript.RegExp"];

/// The names that the project gives to the four references every project of
/// Excel has (VBA, Excel, stdole and Office), besides the name of their type
/// library: the name of the reference is what stands in when the library has
/// none.
const DEFAULT_REFERENCES: [&str; 6] = [
    "visual basic for applications",
    "vba",
    "ole automation",
    "stdole",
    "excel",
    "office",
];

/// A length as a metric: it saturates.
pub(super) fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The metrics of a module whose lines are `lines`, given the metrics of its
/// `procedures`.
///
/// A module that has no procedure has a complexity of 0, not an unknown one.
pub(super) fn module_metrics(lines: &SourceLines, procedures: &[Metrics]) -> Metrics {
    let cyclomatic = procedures.iter().filter_map(|metrics| metrics.cyclomatic);
    let nesting = procedures.iter().filter_map(|metrics| metrics.max_nesting);
    Metrics {
        loc: lines.physical_lines,
        logical_loc: logical_lines(&lines.logical),
        procedures: Some(count(procedures.len())),
        cyclomatic: Some(cyclomatic.clone().max().unwrap_or(0)),
        cyclomatic_total: Some(cyclomatic.fold(0, u32::saturating_add)),
        max_nesting: Some(nesting.max().unwrap_or(0)),
        ..Metrics::default()
    }
}

/// The number of logical lines of a module that are not comment-only.
fn logical_lines(lines: &[LogicalLine]) -> u32 {
    let code = (lines.iter()).filter(|line| line.kind != LogicalKind::CommentOnly);
    count(code.count())
}

/// The metrics of a file, from its modules: the sizes summed, the highest
/// cyclomatic complexity and the deepest nesting of the modules, the sum of
/// their complexities, and the forms. `external_dependencies` is left out: see
/// [`project_metrics`].
pub(super) fn file_metrics(project: &VbaProject) -> Metrics {
    let mut file = Metrics {
        cyclomatic: Some(0),
        cyclomatic_total: Some(0),
        max_nesting: Some(0),
        ..Metrics::default()
    };
    let (mut procedures, mut forms, mut declares) = (0u32, 0u32, 0u32);
    for module in &project.modules {
        let metrics = &module.metrics;
        file.loc = file.loc.saturating_add(metrics.loc);
        file.logical_loc = file.logical_loc.saturating_add(metrics.logical_loc);
        file.cyclomatic = file.cyclomatic.max(metrics.cyclomatic);
        file.cyclomatic_total = file
            .cyclomatic_total
            .map(|total| total.saturating_add(metrics.cyclomatic_total.unwrap_or(0)));
        file.max_nesting = file.max_nesting.max(metrics.max_nesting);
        procedures = procedures.saturating_add(count(module.procedures.len()));
        forms = forms.saturating_add(u32::from(module.module_type == ModuleType::Form));
        let declared =
            (module.declarations.as_ref()).and_then(|declarations| declarations.declare_count);
        declares = declares.saturating_add(declared.unwrap_or(0));
    }
    file.procedures = Some(procedures);
    file.forms = Some(forms);
    file.declare_count = Some(declares);
    file
}

/// The metrics of the project for the rule engine: those of each procedure,
/// completed with the dependencies that it has, and those of the file, which
/// are `file` completed in the same way.
///
/// `file` are the metrics of the file as [`file_metrics`] gives them, and
/// `measured` has an entry for each module of `project`, with the metrics that
/// the lines of the procedures gave.
pub(super) fn project_metrics(
    project: &ProjectInput,
    measured: &[Measured],
    mut file: Metrics,
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
    let has_declare = file.declare_count.is_some_and(|declares| declares > 0);
    file.external_dependencies = Some(file_dependencies(project, has_declare));
    ProjectMetrics { procedures, file }
}

/// The external dependencies of a procedure: the distinct ProgIDs it uses, and
/// the distinct `Declare`d routines it calls.
fn procedure_dependencies(procedure: &ProcedureInput, declared: &NameSet) -> u32 {
    let progids = progids(procedure.lines.iter());
    let called: BTreeSet<String> = (procedure.lines.iter())
        .flat_map(|line| &line.calls)
        .filter(|call| !call.after_dot && declared.contains(&call.name))
        .map(|call| comparable_key(&call.name))
        .collect();
    count(progids.len().saturating_add(called.len()))
}

/// The external dependencies of a file: the distinct ProgIDs it uses, the
/// references that are not the default ones, and one more if it has a
/// `Declare` statement.
fn file_dependencies(project: &ProjectInput, has_declare: bool) -> u32 {
    let lines = project.modules.iter().flat_map(|module| {
        let procedures = module
            .procedures
            .iter()
            .flat_map(|procedure| &procedure.lines);
        module.declarations.iter().chain(procedures)
    });
    let references: BTreeSet<String> = (project.references.iter())
        .filter(|reference| !is_default_reference(reference))
        .map(|reference| reference.trim().to_lowercase())
        .collect();
    let total = progids(lines)
        .len()
        .saturating_add(references.len())
        .saturating_add(usize::from(has_declare));
    count(total)
}

/// The ProgIDs of `lines` that count as dependencies, once each whatever their
/// letter case.
fn progids<'a>(lines: impl Iterator<Item = &'a CodeLine>) -> BTreeSet<String> {
    lines
        .flat_map(|line| &line.progids)
        .filter(|progid| !(COMMON_PROGIDS.iter()).any(|common| common.eq_ignore_ascii_case(progid)))
        .map(|progid| progid.to_lowercase())
        .collect()
}

/// Whether the reference called `name` is one of the four that every project of
/// Excel has. Its display name is that of its type library, such as `Microsoft
/// Excel 16.0 Object Library`, whose version changes.
fn is_default_reference(name: &str) -> bool {
    let name = name.trim().to_lowercase();
    if DEFAULT_REFERENCES.contains(&name.as_str()) {
        return true;
    }
    ["microsoft excel ", "microsoft office "]
        .iter()
        .any(|product| {
            let version =
                (name.strip_prefix(product)).and_then(|rest| rest.strip_suffix(" object library"));
            version.is_some_and(|version| {
                !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CallSite, ModuleInput};
    use crate::vba::split_lines;

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
            predicates: crate::engine::ParserPredicates::default(),
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
    fn a_module_sums_its_lines_and_compares_its_procedures() {
        let lines = split_lines("Option Explicit\n' note\nSub A()\nEnd Sub\nSub B()\nEnd Sub\n")
            .expect("a short module");
        let procedure = |cyclomatic, max_nesting| Metrics {
            cyclomatic: Some(cyclomatic),
            max_nesting: Some(max_nesting),
            ..Metrics::default()
        };

        let metrics = module_metrics(&lines, &[procedure(3, 1), procedure(5, 0)]);

        assert_eq!((metrics.loc, metrics.logical_loc), (6, 5));
        assert_eq!(metrics.procedures, Some(2));
        assert_eq!(metrics.cyclomatic, Some(5));
        assert_eq!(metrics.cyclomatic_total, Some(8));
        assert_eq!(metrics.max_nesting, Some(1));
    }

    #[test]
    fn a_module_without_a_procedure_has_a_complexity_of_zero() {
        let lines = split_lines("Option Explicit\n").expect("a short module");

        let metrics = module_metrics(&lines, &[]);

        assert_eq!(metrics.procedures, Some(0));
        assert_eq!(
            (
                metrics.cyclomatic,
                metrics.cyclomatic_total,
                metrics.max_nesting
            ),
            (Some(0), Some(0), Some(0))
        );
    }

    #[test]
    fn the_default_references_are_not_dependencies() {
        for default in [
            "Visual Basic For Applications",
            "OLE Automation",
            "Microsoft Excel 16.0 Object Library",
            "Microsoft Office 14.0 Object Library",
            "microsoft excel 9 object library",
            "VBA",
            "stdole",
            "Excel",
            "Office",
        ] {
            assert!(is_default_reference(default), "{default}");
        }
        for other in [
            "Microsoft Outlook 16.0 Object Library",
            "Microsoft Office 16.0 Access database engine Object Library",
            "Microsoft Excel Object Library",
            "Microsoft Scripting Runtime",
            "",
        ] {
            assert!(!is_default_reference(other), "{other}");
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
    fn a_file_counts_progids_references_and_declare_statements() {
        let project = ProjectInput {
            references: vec![
                "Visual Basic For Applications".to_owned(),
                "Microsoft Excel 16.0 Object Library".to_owned(),
                "OLE Automation".to_owned(),
                "Microsoft Office 16.0 Object Library".to_owned(),
                "Microsoft Outlook 16.0 Object Library".to_owned(),
                "Microsoft Scripting Runtime".to_owned(),
            ],
            modules: vec![
                module(
                    vec![line(&["Outlook.Application"], &[])],
                    vec![procedure(vec![line(
                        &["Word.Application", "outlook.application"],
                        &[],
                    )])],
                ),
                module(
                    Vec::new(),
                    vec![procedure(vec![line(&["Scripting.Dictionary"], &[])])],
                ),
            ],
        };

        // Two ProgIDs, two other references, and a Declare statement.
        assert_eq!(file_dependencies(&project, true), 5);
        assert_eq!(file_dependencies(&project, false), 4);
        assert_eq!(file_dependencies(&ProjectInput::default(), false), 0);
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
        // The ProgID, and the Declare statement.
        assert_eq!(metrics.file.external_dependencies, Some(2));
    }
}
