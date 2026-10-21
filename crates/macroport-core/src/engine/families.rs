// SPDX-License-Identifier: Apache-2.0
//! Family scores of the procedures (SPEC M3, ACCEPTANCE A4).
//!
//! [`classify`] scores each procedure of a project against the families of
//! `families.yaml`:
//!
//! - A signal adds its weight once per hit, for at most
//!   `max_hits_per_procedure` hits. A signal of module scope is matched over
//!   the whole module, and one of project scope over the whole project; each
//!   adds half its weight per hit to every procedure of the module or of the
//!   project.
//! - The families that scored come best first: by score, then by the number of
//!   distinct signals that matched, then by family id.
//! - A score carries one evidence item for each hit it counts. A hit in the
//!   procedure is cited at its own line, any other hit at the declaration of
//!   the procedure, so that every score cites a line inside the procedure
//!   (ACCEPTANCE A4). When the cap cuts the hits of a signal of module or
//!   project scope, the hits in the procedure are the ones kept.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Range, RangeInclusive};

use super::eval::Evaluator;
use super::hit::{Hit, Place};
use super::input::{ProcedureInput, ProjectInput};
use crate::report::{Evidence, FamilyScore};
use crate::rules::{Families, FamilySignal, Scope};

#[cfg(test)]
mod tests;

/// The family scores of every procedure of a project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFamilies {
    /// `modules[m][p]` is the procedure `p` of the module `m` of the
    /// [`ProjectInput`].
    pub modules: Vec<Vec<ProcedureFamilies>>,
}

/// The family scores of a procedure.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureFamilies {
    /// The scores, best first, each with the evidence of the hits it counts. A
    /// family with no counted hit is not listed.
    pub families: Vec<FamilyScore>,
}

/// Scores the procedures of `project` against `rules`.
///
/// Pure and deterministic: ties are broken as the module documentation says,
/// and the lists come out in the order of their scores.
pub fn classify(project: &ProjectInput, rules: &Families) -> ProjectFamilies {
    let scorer = Scorer::new(project, rules);
    let mut modules = Vec::with_capacity(project.modules.len());
    for (module, owner) in project.modules.iter().enumerate() {
        let module_hits = scorer.module_hits(module);
        let mut procedures = Vec::with_capacity(owner.procedures.len());
        for (index, procedure) in owner.procedures.iter().enumerate() {
            let at = Located {
                module,
                index,
                procedure,
            };
            procedures.push(procedure_families(scorer.score(at, &module_hits)));
        }
        modules.push(procedures);
    }
    ProjectFamilies { modules }
}

/// A procedure of the project, and where to find it.
#[derive(Clone, Copy)]
struct Located<'p> {
    module: usize,
    index: usize,
    procedure: &'p ProcedureInput,
}

impl Located<'_> {
    /// The lines of the procedure, as places of its module.
    fn span(&self) -> RangeInclusive<Option<Place>> {
        let module = self.module;
        let place = |line| Some(Place { module, line });
        place(self.procedure.start_line)..=place(self.procedure.end_line)
    }
}

/// The rules and the hits that are the same for every procedure.
struct Scorer<'a> {
    evaluator: Evaluator<'a>,
    /// Every signal with the id of its family, in the order of the rules.
    signals: Vec<(&'a str, &'a FamilySignal)>,
    /// The hits of the signals of project scope, aligned with `signals`; empty
    /// for the signals of another scope.
    project_hits: Vec<Vec<Hit<'a>>>,
}

impl<'a> Scorer<'a> {
    fn new(project: &'a ProjectInput, rules: &'a Families) -> Self {
        let signals = rules.families.iter().flat_map(|family| {
            let id = family.id.as_str();
            family.signals.iter().map(move |signal| (id, signal))
        });
        let mut scorer = Self {
            evaluator: Evaluator::for_families(project, rules),
            signals: signals.collect(),
            project_hits: Vec::new(),
        };
        scorer.project_hits = scorer.hits_of_scope(Scope::Project, |evaluator, signal| {
            evaluator.project_hits(&signal.matcher, signal.text)
        });
        scorer
    }

    /// The hits of the signals of module scope in `module`, aligned with
    /// `signals`; empty for the signals of another scope.
    fn module_hits(&self, module: usize) -> Vec<Vec<Hit<'a>>> {
        self.hits_of_scope(Scope::Module, |evaluator, signal| {
            evaluator.module_hits(module, &signal.matcher, signal.text)
        })
    }

    /// The hits that `hits` finds for each signal of `scope`, aligned with
    /// `signals`; empty for the signals of another scope.
    fn hits_of_scope(
        &self,
        scope: Scope,
        hits: impl Fn(&Evaluator<'a>, &FamilySignal) -> Vec<Hit<'a>>,
    ) -> Vec<Vec<Hit<'a>>> {
        let for_signal = |&(_, signal): &(&str, &FamilySignal)| {
            if signal.scope == scope {
                hits(&self.evaluator, signal)
            } else {
                Vec::new()
            }
        };
        self.signals.iter().map(for_signal).collect()
    }

    /// The scores of the procedure `at`. `module_hits` is `self.module_hits`
    /// for its module.
    fn score(&self, at: Located<'_>, module_hits: &[Vec<Hit<'a>>]) -> Tallies<'a> {
        let mut tallies = Tallies::default();
        for (position, &(family, signal)) in self.signals.iter().enumerate() {
            let own;
            let weight = u64::from(signal.weight);
            // The points of a hit, in half points: a hit of a module or
            // project signal adds half the weight.
            let (hits, inside, per_hit) = match signal.scope {
                Scope::Procedure => {
                    let (module, index) = (at.module, at.index);
                    own =
                        self.evaluator
                            .procedure_hits(module, index, &signal.matcher, signal.text);
                    (own.as_slice(), 0..own.len(), weight.saturating_mul(2))
                }
                Scope::Module => {
                    let hits = hits_at(module_hits, position);
                    (hits, hits_inside(hits, at), weight)
                }
                Scope::Project => {
                    let hits = hits_at(&self.project_hits, position);
                    (hits, hits_inside(hits, at), weight)
                }
            };
            let cap = usize::try_from(signal.max_hits_per_procedure).unwrap_or(usize::MAX);
            let counted = counted_hits(hits, &inside, cap);
            if counted.is_empty() {
                continue;
            }
            let cited = counted.iter().map(|hit| evidence(&signal.id, hit, at));
            let tally = tallies.0.entry(family).or_default();
            tally.count(&signal.id, per_hit, cited.collect());
        }
        tallies
    }
}

/// The hits of the signal at `position` in a table aligned with the signals.
fn hits_at<'h, 'a>(table: &'h [Vec<Hit<'a>>], position: usize) -> &'h [Hit<'a>] {
    table.get(position).map_or(&[], Vec::as_slice)
}

/// The range of `hits`, which are sorted by place, that lie in the procedure.
fn hits_inside(hits: &[Hit<'_>], at: Located<'_>) -> Range<usize> {
    let span = at.span();
    // A hit with no place (a reference) sorts before every place.
    let first = hits.partition_point(|hit| hit.place < *span.start());
    // A procedure that ends before it starts holds nothing.
    first
        ..hits
            .partition_point(|hit| hit.place <= *span.end())
            .max(first)
}

/// The hits that a signal counts among `hits`: at most `cap`. The number is all
/// that the score reads. The choice only decides what is cited, so those at the
/// positions `inside` are counted first, for the procedure to cite its own
/// lines before the declaration of the procedure stands in for the others.
fn counted_hits<'h, 'a>(
    hits: &'h [Hit<'a>],
    inside: &Range<usize>,
    cap: usize,
) -> Vec<&'h Hit<'a>> {
    let part = |range: Range<usize>| hits.get(range).unwrap_or_default().iter();
    part(inside.clone())
        .chain(part(0..inside.start))
        .chain(part(inside.end..hits.len()))
        .take(cap)
        .collect()
}

/// The evidence of a hit counted for the procedure `at`: the line of a hit in
/// the procedure, the declaration of the procedure for any other (a statement
/// of the module outside the procedure, a line of another module, a reference
/// that has no line). The snippet quotes the hit in every case. Built as
/// [`Hit::evidence`] does, which has no line to give for those hits.
fn evidence(rule_id: &str, hit: &Hit<'_>, at: Located<'_>) -> Evidence {
    let line = match hit.place {
        Some(place) if at.span().contains(&Some(place)) => place.line,
        _ => at.procedure.start_line,
    };
    Evidence {
        rule_id: rule_id.to_owned(),
        line: line.max(1),
        snippet: Some(hit.snippet()),
    }
}

/// What the signals gave a family: half points, to add up exactly, the signals
/// that matched and the evidence.
#[derive(Debug, Default)]
struct Tally<'a> {
    half_points: u64,
    signals: BTreeSet<&'a str>,
    evidence: Vec<Evidence>,
}

impl<'a> Tally<'a> {
    /// Adds the hits of the signal `id`, each worth `per_hit` half points: the
    /// `evidence` has one item for each.
    fn count(&mut self, id: &'a str, per_hit: u64, evidence: Vec<Evidence>) {
        let hits = u64::try_from(evidence.len()).unwrap_or(u64::MAX);
        let points = per_hit.saturating_mul(hits);
        self.half_points = self.half_points.saturating_add(points);
        self.signals.insert(id);
        self.evidence.extend(evidence);
    }

    /// The score of `family`, with its evidence in the order of the lines.
    fn into_score(mut self, family: &str) -> FamilyScore {
        let by_line = |a: &Evidence, b: &Evidence| (a.line, &a.rule_id).cmp(&(b.line, &b.rule_id));
        self.evidence.sort_by(by_line);
        FamilyScore {
            family: family.to_owned(),
            score: points(self.half_points),
            evidence: Some(self.evidence),
        }
    }
}

/// The tallies of a procedure, by family id.
#[derive(Debug, Default)]
struct Tallies<'a>(BTreeMap<&'a str, Tally<'a>>);

impl<'a> Tallies<'a> {
    /// The families, best first: the higher score, then the more distinct
    /// signals, then the lower family id.
    fn ranked(self) -> Vec<(&'a str, Tally<'a>)> {
        let mut ranked: Vec<_> = self.0.into_iter().collect();
        ranked.sort_by_key(|(id, tally)| {
            (
                Reverse(tally.half_points),
                Reverse(tally.signals.len()),
                *id,
            )
        });
        ranked
    }
}

/// A score in half points as the score of the report. An `f64` holds every
/// whole number below 2^53 exactly, far above the half points of any rules.
fn points(half_points: u64) -> f64 {
    half_points as f64 / 2.0
}

fn procedure_families(tallies: Tallies<'_>) -> ProcedureFamilies {
    let families = tallies
        .ranked()
        .into_iter()
        .map(|(family, tally)| tally.into_score(family))
        .collect();
    ProcedureFamilies { families }
}
