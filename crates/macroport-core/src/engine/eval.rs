// SPDX-License-Identifier: Apache-2.0
//! The evaluator: runs a matcher over a procedure, a module or a project.

use super::hit::Hit;
use super::input::{CodeLine, ProcedureInput, ProjectInput};
use super::matchers;
use crate::rules::{Matcher, TextMode};

/// Evaluates matchers over one project.
///
/// The three methods give the hits of a signal by its scope (SPEC section 7).
/// A module-scope or project-scope signal is evaluated once, not once per
/// procedure: callers keep the result and give it to each procedure.
///
/// Only `member` is implemented so far: the other matchers give no hit yet.
#[derive(Debug, Clone, Copy)]
pub struct Evaluator<'p> {
    project: &'p ProjectInput,
}

impl<'p> Evaluator<'p> {
    /// An evaluator over `project`.
    pub fn new(project: &'p ProjectInput) -> Self {
        Self { project }
    }

    /// The hits of `matcher` in the procedure `procedure` of the module
    /// `module` (indexes in `ProjectInput::modules` and
    /// `ModuleInput::procedures`), from its declaration to its `End` line. No
    /// hit when an index is out of range.
    ///
    /// `text` is the `text` key of the signal: only `regex` reads it.
    pub fn procedure_hits(
        &self,
        module: usize,
        procedure: usize,
        matcher: &Matcher,
        text: TextMode,
    ) -> Vec<Hit<'p>> {
        self.hits(Target::Procedure { module, procedure }, matcher, text)
    }

    /// The hits of `matcher` in the module `module`: its declarations and all
    /// its procedures. No hit when `module` is out of range.
    pub fn module_hits(&self, module: usize, matcher: &Matcher, text: TextMode) -> Vec<Hit<'p>> {
        self.hits(Target::Module(module), matcher, text)
    }

    /// The hits of `matcher` in the whole project.
    pub fn project_hits(&self, matcher: &Matcher, text: TextMode) -> Vec<Hit<'p>> {
        self.hits(Target::Project, matcher, text)
    }

    /// The hits in the order of the source: by module, then by line, and
    /// within a line in the order the matcher found them.
    fn hits(&self, target: Target, matcher: &Matcher, text: TextMode) -> Vec<Hit<'p>> {
        let units = self.units(target);
        let mut hits = Vec::new();
        match matcher {
            Matcher::Member(names) => matchers::member(&units, names, &mut hits),
            // The other matchers arrive with the next changes.
            Matcher::Chain(_)
            | Matcher::Call(_)
            | Matcher::ProgId(_)
            | Matcher::ProcedureName(_)
            | Matcher::ModuleType(_)
            | Matcher::Reference(_)
            | Matcher::Regex(_)
            | Matcher::String(_)
            | Matcher::Comment(_)
            | Matcher::Structural(_) => {
                let _ = text;
            }
        }
        // Declarations and procedures interleave in the source.
        hits.sort_by_key(|hit| hit.place);
        hits
    }

    /// The runs of lines that `target` covers.
    fn units(&self, target: Target) -> Vec<Unit<'p>> {
        match target {
            Target::Procedure { module, procedure } => self
                .project
                .modules
                .get(module)
                .and_then(|owner| owner.procedures.get(procedure))
                .map(|procedure| Unit::of_procedure(module, procedure))
                .into_iter()
                .collect(),
            Target::Module(module) => self.module_units(module),
            Target::Project => (0..self.project.modules.len())
                .flat_map(|module| self.module_units(module))
                .collect(),
        }
    }

    /// The declarations of a module, then its procedures.
    fn module_units(&self, module: usize) -> Vec<Unit<'p>> {
        let Some(owner) = self.project.modules.get(module) else {
            return Vec::new();
        };
        let declarations = Unit {
            module,
            lines: &owner.declarations,
        };
        std::iter::once(declarations)
            .chain(
                owner
                    .procedures
                    .iter()
                    .map(|procedure| Unit::of_procedure(module, procedure)),
            )
            .collect()
    }
}

/// What a signal is evaluated over.
#[derive(Debug, Clone, Copy)]
enum Target {
    Procedure { module: usize, procedure: usize },
    Module(usize),
    Project,
}

/// A run of logical lines: the declarations of a module, or a procedure.
#[derive(Debug, Clone, Copy)]
pub(super) struct Unit<'p> {
    /// Index of the module in the project.
    pub module: usize,
    pub lines: &'p [CodeLine],
}

impl<'p> Unit<'p> {
    fn of_procedure(module: usize, procedure: &'p ProcedureInput) -> Self {
        Self {
            module,
            lines: &procedure.lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::testing::{line, module, procedure, project};
    use crate::rules::ModuleType;

    fn foo(number: u32) -> CodeLine {
        line(number, "Foo.Bar").member("Foo")
    }

    /// Two modules, each statement using `Foo` once. `Module1`: a declaration
    /// at line 2, `First` from line 4 and `Second` from line 8. `Class1`:
    /// `Third` from line 3.
    fn sample() -> ProjectInput {
        project(vec![
            module(
                "Module1",
                ModuleType::Standard,
                vec![
                    procedure("First", 4, vec![foo(5)]),
                    procedure("Second", 8, vec![foo(9)]),
                ],
            )
            .with_declarations(vec![foo(2)]),
            module(
                "Class1",
                ModuleType::Class,
                vec![procedure("Third", 3, vec![foo(4)])],
            ),
        ])
    }

    fn matcher() -> Matcher {
        Matcher::Member(vec!["Foo".to_owned()])
    }

    /// The module and the line of each hit.
    fn places(hits: &[Hit<'_>]) -> Vec<(usize, u32)> {
        hits.iter()
            .filter_map(|hit| hit.place)
            .map(|place| (place.module, place.line))
            .collect()
    }

    #[test]
    fn a_procedure_scope_reads_that_procedure_only() {
        let project = sample();
        let evaluator = Evaluator::new(&project);
        let hits = |module, procedure| {
            places(&evaluator.procedure_hits(module, procedure, &matcher(), TextMode::Code))
        };
        assert_eq!(hits(0, 0), [(0, 5)]);
        assert_eq!(hits(0, 1), [(0, 9)]);
        assert_eq!(hits(1, 0), [(1, 4)]);
    }

    #[test]
    fn a_module_scope_reads_the_declarations_and_every_procedure() {
        let project = sample();
        let evaluator = Evaluator::new(&project);
        let hits = |module| places(&evaluator.module_hits(module, &matcher(), TextMode::Code));
        assert_eq!(hits(0), [(0, 2), (0, 5), (0, 9)]);
        assert_eq!(hits(1), [(1, 4)]);
    }

    #[test]
    fn a_project_scope_reads_every_module() {
        let project = sample();
        let hits = Evaluator::new(&project).project_hits(&matcher(), TextMode::Code);
        assert_eq!(places(&hits), [(0, 2), (0, 5), (0, 9), (1, 4)]);
    }

    #[test]
    fn hits_come_in_the_order_of_the_source() {
        // Declarations that follow the procedures, as a module may have them.
        let project = project(vec![
            module(
                "Module1",
                ModuleType::Standard,
                vec![procedure("First", 4, vec![foo(5)])],
            )
            .with_declarations(vec![foo(20)]),
        ]);
        let hits = Evaluator::new(&project).module_hits(0, &matcher(), TextMode::Code);
        assert_eq!(places(&hits), [(0, 5), (0, 20)]);
    }

    #[test]
    fn an_index_out_of_range_gives_no_hit() {
        let project = sample();
        let evaluator = Evaluator::new(&project);
        let text = TextMode::Code;
        assert!(evaluator.procedure_hits(0, 2, &matcher(), text).is_empty());
        assert!(evaluator.procedure_hits(2, 0, &matcher(), text).is_empty());
        assert!(evaluator.module_hits(2, &matcher(), text).is_empty());
        let empty = ProjectInput::default();
        let evaluator = Evaluator::new(&empty);
        assert!(evaluator.project_hits(&matcher(), text).is_empty());
    }
}
