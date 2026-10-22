// SPDX-License-Identifier: Apache-2.0
//! The evaluator: runs a matcher over a procedure, a module or a project.

use super::glob::glob_matches;
use super::hit::Hit;
use super::input::ProjectInput;
use super::matchers;
use super::unit::Unit;
use crate::rules::{Families, Matcher, TextMode};

/// Evaluates matchers over one project.
///
/// The three methods give the hits of a signal by its scope (SPEC section 7).
/// A module-scope or project-scope signal is evaluated once, not once per
/// procedure: callers keep the result and give it to each procedure.
///
/// `member`, `chain`, `call` and `progid` are implemented so far: the other
/// matchers give no hit yet.
#[derive(Debug, Clone, Copy)]
pub struct Evaluator<'p> {
    project: &'p ProjectInput,
    /// The family rules, when the `*` ProgID is the one of the families.
    families: Option<&'p Families>,
}

impl<'p> Evaluator<'p> {
    /// An evaluator over `project` for the security signals and the traits: a
    /// `progid` pattern `*` matches any ProgID (SPEC section 7).
    pub fn new(project: &'p ProjectInput) -> Self {
        Self {
            project,
            families: None,
        }
    }

    /// An evaluator over `project` for the family signals: a `progid` pattern
    /// `*` matches only a ProgID that no other `progid` pattern of `families`
    /// matches, so that `win.other-com` counts the COM objects that no other
    /// signal accounts for (SPEC section 7).
    pub fn for_families(project: &'p ProjectInput, families: &'p Families) -> Self {
        Self {
            project,
            families: Some(families),
        }
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
            Matcher::Chain(patterns) => matchers::chain(&units, patterns, &mut hits),
            Matcher::Call(names) => matchers::call(&units, names, &mut hits),
            Matcher::ProgId(patterns) => {
                let catch_all = |progid: &str| self.unclaimed(progid);
                matchers::progid(&units, patterns, catch_all, &mut hits);
            }
            // The other matchers arrive with the next changes. `text` is for
            // `regex`, and is unread until then.
            Matcher::ProcedureName(_)
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

    /// Whether a `progid` pattern `*` matches `progid`: always, except for the
    /// families, where no other `progid` pattern may match it.
    fn unclaimed(&self, progid: &str) -> bool {
        self.families.is_none_or(|families| {
            !claimed_patterns(families).any(|pattern| glob_matches(pattern, progid))
        })
    }

    /// The runs of lines that `target` covers.
    fn units(&self, target: Target) -> Vec<Unit<'p>> {
        match target {
            Target::Procedure { module, procedure } => {
                self.procedure_unit(module, procedure).into_iter().collect()
            }
            Target::Module(module) => self.module_units(module),
            Target::Project => (0..self.project.modules.len())
                .flat_map(|module| self.module_units(module))
                .collect(),
        }
    }

    /// The unit of a procedure, if both indexes are in range.
    fn procedure_unit(&self, module: usize, procedure: usize) -> Option<Unit<'p>> {
        let found = self
            .project
            .modules
            .get(module)?
            .procedures
            .get(procedure)?;
        Some(Unit::of_procedure(module, found))
    }

    /// The units of a module, if the index is in range.
    fn module_units(&self, module: usize) -> Vec<Unit<'p>> {
        let owner = self.project.modules.get(module);
        owner.map_or_else(Vec::new, |owner| Unit::of_module(module, owner))
    }
}

/// What a signal is evaluated over: the scope of the signal, with the indexes
/// it applies to.
#[derive(Debug, Clone, Copy)]
enum Target {
    Procedure { module: usize, procedure: usize },
    Module(usize),
    Project,
}

/// The `progid` patterns of the signals of `families`, but the catch-all `*`.
fn claimed_patterns(families: &Families) -> impl Iterator<Item = &str> {
    families
        .families
        .iter()
        .flat_map(|family| &family.signals)
        .filter_map(|signal| match &signal.matcher {
            Matcher::ProgId(patterns) => Some(patterns),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .filter(|pattern| *pattern != matchers::ANY_PROGID)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CodeLine;
    use crate::engine::testing::{line, members, module, procedure, project};
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
        members(&["Foo"])
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
    fn a_procedure_is_read_from_its_declaration_to_its_end_line() {
        // The type of a parameter is a member of the declaration. The `End`
        // line has none in real code: it holds one here to show it is read.
        let mut first = procedure("First", 4, Vec::new());
        first.lines = vec![
            line(4, "Sub First(f As Foo)").member("Foo"),
            foo(5),
            line(6, "End Sub").member("Foo"),
        ];
        let project = project(vec![module("Module1", ModuleType::Standard, vec![first])]);
        let hits = Evaluator::new(&project).procedure_hits(0, 0, &matcher(), TextMode::Code);
        assert_eq!(places(&hits), [(0, 4), (0, 5), (0, 6)]);
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
