// SPDX-License-Identifier: Apache-2.0
//! The runs of lines that a matcher reads.

use super::input::{CodeLine, ModuleInput, ProcedureInput};

/// A run of logical lines: the declarations of a module, or one of its
/// procedures.
#[derive(Debug, Clone, Copy)]
pub(super) struct Unit<'p> {
    /// Index of the module in the project.
    pub module: usize,
    pub lines: &'p [CodeLine],
}

impl<'p> Unit<'p> {
    /// The units of a module: its declarations, then each of its procedures.
    pub(super) fn of_module(module: usize, owner: &'p ModuleInput) -> Vec<Self> {
        let declarations = Self {
            module,
            lines: &owner.declarations,
        };
        std::iter::once(declarations)
            .chain(
                owner
                    .procedures
                    .iter()
                    .map(|procedure| Self::of_procedure(module, procedure)),
            )
            .collect()
    }

    /// The unit of a procedure, from its declaration to its `End` line.
    pub(super) fn of_procedure(module: usize, procedure: &'p ProcedureInput) -> Self {
        Self {
            module,
            lines: &procedure.lines,
        }
    }
}
