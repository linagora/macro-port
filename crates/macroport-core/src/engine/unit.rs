// SPDX-License-Identifier: Apache-2.0
//! The runs of lines that a matcher reads.

use super::input::{CodeLine, ModuleInput, ProcedureInput};

/// A run of logical lines: the declarations of a module, or one of its
/// procedures.
#[derive(Debug, Clone, Copy)]
pub(super) struct Unit<'p> {
    /// Index of `owner` in the project: the `module` of the places of the hits.
    pub module: usize,
    /// The module that holds the lines.
    pub owner: &'p ModuleInput,
    /// The procedure, or `None` for the declarations of the module.
    pub procedure: Option<&'p ProcedureInput>,
}

impl<'p> Unit<'p> {
    /// The units of a module: its declarations, then each of its procedures. A
    /// module that exists always has the first, even with no declaration, so
    /// that it is never without a unit.
    pub(super) fn of_module(module: usize, owner: &'p ModuleInput) -> Vec<Self> {
        let declarations = Self {
            module,
            owner,
            procedure: None,
        };
        std::iter::once(declarations)
            .chain(
                owner
                    .procedures
                    .iter()
                    .map(|procedure| Self::of_procedure(module, owner, procedure)),
            )
            .collect()
    }

    /// The unit of a procedure, from its declaration to its `End` line.
    pub(super) fn of_procedure(
        module: usize,
        owner: &'p ModuleInput,
        procedure: &'p ProcedureInput,
    ) -> Self {
        Self {
            module,
            owner,
            procedure: Some(procedure),
        }
    }

    /// The lines of the unit.
    pub(super) fn lines(&self) -> &'p [CodeLine] {
        match self.procedure {
            Some(procedure) => &procedure.lines,
            None => &self.owner.declarations,
        }
    }
}
