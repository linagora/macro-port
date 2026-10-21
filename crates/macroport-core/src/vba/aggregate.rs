// SPDX-License-Identifier: Apache-2.0
//! The metrics of modules and files (SPEC M2 item 7): what the metrics of the
//! procedures of a module add up to, and those of the modules of a file, with
//! the external dependencies of a file (see the `dependencies` module).
//!
//! The functions add up what the caller has measured. A metric that a
//! procedure leaves out counts as 0 in a sum and in a maximum, and the level
//! above still reports its own: a module with no procedure has a cyclomatic
//! complexity of 0.

use super::SourceLines;
use super::dependencies::{count_progids, count_references};
use super::metrics::logical_lines;
use crate::report::{Metrics, ModuleReport, VbaProject};
use crate::rules::ModuleType;

/// What the metrics of some procedures add up to.
#[derive(Debug, Default)]
struct Sums {
    procedures: u32,
    /// The highest cyclomatic complexity.
    cyclomatic: u32,
    /// The sum of the cyclomatic complexities.
    cyclomatic_total: u32,
    /// The deepest nesting.
    max_nesting: u32,
}

impl Sums {
    fn of<'a>(procedures: impl IntoIterator<Item = &'a Metrics>) -> Self {
        let mut sums = Self::default();
        for metrics in procedures {
            let cyclomatic = metrics.cyclomatic.unwrap_or_default();
            sums.procedures = sums.procedures.saturating_add(1);
            sums.cyclomatic = sums.cyclomatic.max(cyclomatic);
            sums.cyclomatic_total = sums.cyclomatic_total.saturating_add(cyclomatic);
            sums.max_nesting = sums
                .max_nesting
                .max(metrics.max_nesting.unwrap_or_default());
        }
        sums
    }

    /// The metrics that a module and a file both report: their sizes, `loc`
    /// physical lines of which `logical_loc` are code, and these sums.
    fn metrics(self, loc: u32, logical_loc: u32) -> Metrics {
        Metrics {
            loc,
            logical_loc,
            procedures: Some(self.procedures),
            cyclomatic: Some(self.cyclomatic),
            cyclomatic_total: Some(self.cyclomatic_total),
            max_nesting: Some(self.max_nesting),
            ..Metrics::default()
        }
    }
}

/// The metrics of a module: the sizes of its text, which `lines` gives, and
/// what the `procedures` that the module holds add up to (SPEC M2 item 7).
/// `procedures` are the metrics of the procedures, `measure_procedure` giving
/// each. `loc` and `logical_loc` are those of the text of the module, not a sum
/// over its procedures: they include the declarations before them. `cyclomatic`
/// is the highest of the procedures and `cyclomatic_total` their sum,
/// `max_nesting` is the deepest, and a module with no procedure has 0 for each.
///
/// These are the metrics that `schema/examples/report.example.json` gives a
/// module. `forms` and `external_dependencies` belong to the file. The count of
/// `Declare` statements of a module is left out too: the example has it in the
/// `declarations` section of the module, which the caller fills from the
/// declarations that the parser found.
pub fn module_metrics<'a>(
    lines: &SourceLines,
    procedures: impl IntoIterator<Item = &'a Metrics>,
) -> Metrics {
    Sums::of(procedures).metrics(lines.physical_lines, logical_lines(&lines.logical))
}

/// The metrics of a file (SPEC M2 item 7). `loc` and `logical_loc` add up those
/// of its modules. `procedures`, `cyclomatic` (the highest), `cyclomatic_total`
/// (the sum) and `max_nesting` (the deepest) are those of all the procedures
/// the modules report. `forms` counts the form modules.
///
/// `declare_count` adds up the count of `Declare` statements of each module,
/// which is the one of its declarations section, or else its metric of that
/// name; a module that has neither counts 0. `external_dependencies` is the
/// number of distinct ProgIDs among `progids`, plus the number of references of
/// the project that every project of Excel does not have, plus one if any
/// `Declare` statement exists (`rules/complexity.yaml`). `progids` are the
/// strings that the code of the whole file passes to `CreateObject` and
/// `GetObject`, after constant folding; a string that is not shaped like a
/// ProgID, and `Scripting.Dictionary` and `VBScript.RegExp`, are not counted.
pub fn file_metrics<I>(project: &VbaProject, progids: I) -> Metrics
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let procedures = (project.modules.iter())
        .flat_map(|module| &module.procedures)
        .map(|procedure| &procedure.metrics);
    let (mut loc, mut logical_loc, mut forms, mut declares) = (0_u32, 0_u32, 0_u32, 0_u32);
    for module in &project.modules {
        loc = loc.saturating_add(module.metrics.loc);
        logical_loc = logical_loc.saturating_add(module.metrics.logical_loc);
        forms = forms.saturating_add(u32::from(module.module_type == ModuleType::Form));
        declares = declares.saturating_add(declare_statements(module));
    }
    let references = project.references.as_deref().unwrap_or_default();
    let dependencies = count_progids(progids)
        .saturating_add(count_references(references))
        .saturating_add(u32::from(declares > 0));
    Metrics {
        forms: Some(forms),
        declare_count: Some(declares),
        external_dependencies: Some(dependencies),
        ..Sums::of(procedures).metrics(loc, logical_loc)
    }
}

/// The count of `Declare` statements that a module reports: in its declarations
/// section, where `schema/examples/report.example.json` has it, or else in its
/// metrics, where the pipeline also puts it. 0 if it reports none.
fn declare_statements(module: &ModuleReport) -> u32 {
    (module.declarations.as_ref())
        .and_then(|section| section.declare_count)
        .or(module.metrics.declare_count)
        .unwrap_or_default()
}
