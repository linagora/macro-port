// SPDX-License-Identifier: Apache-2.0
//! The decisions of the rule engine, in the report of a file.
//!
//! [`apply`] copies what [`Findings`] says into the fields of the report that
//! the rules decide: the families, the signals, the traits and the level. They
//! replace what the front end had put there, since the engine decides the traits
//! that the parser's predicates give as well.

use super::input::Measured;
use crate::engine::findings::Findings;
use crate::report::FileReport;

/// Writes `findings` into `file`. `measured` has an entry for each module of the
/// project of the file, as `findings` does: a module that the front end did not
/// read keeps the report it had, since nothing was measured of it.
///
/// The metrics of the report are not touched: the front end gave them, and the
/// engine read the same ones, but for the dependencies that the report leaves
/// out.
///
/// `judged` tells whether a module was read. A file where none was has only the
/// signals of its container indicators, and the severity they give it: its
/// families, its level and its traits would be those of an empty project, which
/// nothing measured.
pub(super) fn apply(
    file: &mut FileReport,
    findings: Findings,
    measured: &[Measured],
    judged: bool,
) {
    let Findings {
        modules,
        file: decided,
    } = findings;
    if let Some(project) = file.vba_project.as_mut() {
        let found = (project.modules.iter_mut().zip(modules)).zip(measured);
        for ((module, found), measured) in found {
            if !measured.read {
                continue;
            }
            module.signals = Some(found.signals);
            for (procedure, decided) in module.procedures.iter_mut().zip(found.procedures) {
                procedure.primary_family = decided.primary_family;
                procedure.secondary_families = Some(decided.secondary_families);
                procedure.families = decided.families;
                procedure.signals = decided.signals;
                procedure.traits = Some(decided.traits);
            }
        }
    }
    file.max_severity = decided.max_severity;
    file.signals = Some(decided.signals);
    if judged {
        file.complexity_level = Some(decided.complexity_level);
        file.primary_family = Some(decided.primary_family);
        file.families = Some(decided.families);
        file.traits = Some(decided.traits);
    }
}
