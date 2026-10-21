// SPDX-License-Identifier: Apache-2.0
//! The decisions of the rule engine, in the report of a file.
//!
//! [`apply`] copies what [`Findings`] says into the fields of the report that
//! the rules decide: the families, the signals, the traits and the level. They
//! replace what the front end had put there, since the engine decides the traits
//! that the parser's predicates give as well.

use super::input::Measured;
use crate::engine::findings::{Findings, ProjectMetrics};
use crate::report::FileReport;

/// Writes `findings` into `file`, and the metrics of the procedures that the
/// engine read. `measured` has an entry for each module of the project of the
/// file, as `findings` does: a module that the front end did not read keeps the
/// report it had, since nothing was measured of it.
pub(super) fn apply(
    file: &mut FileReport,
    findings: Findings,
    metrics: ProjectMetrics,
    measured: &[Measured],
) {
    let Findings {
        modules,
        file: decided,
    } = findings;
    if let Some(project) = file.vba_project.as_mut() {
        let found = (project.modules.iter_mut().zip(modules))
            .zip(metrics.procedures)
            .zip(measured);
        for (((module, found), procedure_metrics), measured) in found {
            if !measured.read {
                continue;
            }
            module.signals = Some(found.signals);
            let found = (module.procedures.iter_mut().zip(found.procedures)).zip(procedure_metrics);
            for ((procedure, decided), metrics) in found {
                procedure.metrics = metrics;
                procedure.primary_family = decided.primary_family;
                procedure.secondary_families = Some(decided.secondary_families);
                procedure.families = decided.families;
                procedure.signals = decided.signals;
                procedure.traits = Some(decided.traits);
            }
        }
    }
    file.complexity_level = Some(decided.complexity_level);
    file.primary_family = Some(decided.primary_family);
    file.families = Some(decided.families);
    file.max_severity = decided.max_severity;
    file.signals = Some(decided.signals);
    file.traits = Some(decided.traits);
}
