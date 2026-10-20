// SPDX-License-Identifier: Apache-2.0
//! Projects written by hand, for the tests of the engine and of the code that
//! builds on it.

use std::collections::BTreeSet;

use super::input::{
    CodeLine, MemberUse, ModuleInput, ParserPredicates, ProcedureInput, ProjectInput,
};
use crate::report::ProcedureKind;
use crate::rules::{Matcher, ModuleType};

/// A statement on the physical line `number`, with nothing found in it yet:
/// the builder methods add what the parser would have found.
pub(crate) fn line(number: u32, raw: &str) -> CodeLine {
    CodeLine {
        line: number,
        raw: raw.to_owned(),
        ..CodeLine::default()
    }
}

impl CodeLine {
    /// Adds an identifier used as a member or as a standalone name.
    pub(crate) fn member(mut self, name: &str) -> Self {
        self.members.push(MemberUse {
            name: name.to_owned(),
            named_argument: false,
        });
        self
    }

    /// Adds an identifier that names an argument, as in `Name:=value`.
    pub(crate) fn named_argument(mut self, name: &str) -> Self {
        self.members.push(MemberUse {
            name: name.to_owned(),
            named_argument: true,
        });
        self
    }
}

/// A `Sub` of the given name on the physical line `start_line`, holding
/// `body`. The declaration and the `End Sub` line are added around it.
pub(crate) fn procedure(name: &str, start_line: u32, body: Vec<CodeLine>) -> ProcedureInput {
    let end_line = body
        .last()
        .map_or(start_line, |last| last.line)
        .saturating_add(1);
    let mut lines = vec![line(start_line, &format!("Sub {name}()"))];
    lines.extend(body);
    lines.push(line(end_line, "End Sub"));
    ProcedureInput {
        name: name.to_owned(),
        kind: ProcedureKind::Sub,
        start_line,
        end_line,
        lines,
        predicates: ParserPredicates::default(),
    }
}

/// A module with these procedures and nothing else.
pub(crate) fn module(
    name: &str,
    module_type: ModuleType,
    procedures: Vec<ProcedureInput>,
) -> ModuleInput {
    ModuleInput {
        name: name.to_owned(),
        module_type,
        with_events: BTreeSet::new(),
        controls: BTreeSet::new(),
        declarations: Vec::new(),
        procedures,
    }
}

impl ModuleInput {
    /// Sets the lines outside the procedures.
    pub(crate) fn with_declarations(mut self, declarations: Vec<CodeLine>) -> Self {
        self.declarations = declarations;
        self
    }
}

/// A project of these modules, with no reference.
pub(crate) fn project(modules: Vec<ModuleInput>) -> ProjectInput {
    ProjectInput {
        references: Vec::new(),
        modules,
    }
}

/// A project of one standard module that holds one procedure, whose body is
/// `body`.
pub(crate) fn project_of(body: &[CodeLine]) -> ProjectInput {
    project(vec![module(
        "Module1",
        ModuleType::Standard,
        vec![procedure("Main", 1, body.to_vec())],
    )])
}

/// A `member` matcher for these names.
pub(crate) fn members(names: &[&str]) -> Matcher {
    Matcher::Member(names.iter().map(|name| (*name).to_owned()).collect())
}
