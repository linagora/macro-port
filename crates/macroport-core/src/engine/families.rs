// SPDX-License-Identifier: Apache-2.0
//! Family scores and primary families of the procedures and of the file (SPEC
//! M3, ACCEPTANCE A4).
//!
//! [`classify`] scores each procedure of a project against the families of
//! `families.yaml`, then scores the file:
//!
//! - A signal adds its weight once per hit, for at most
//!   `max_hits_per_procedure` hits. A signal of module scope is matched over
//!   the whole module, and one of project scope over the whole project; each
//!   adds half its weight per hit to every procedure of the module or of the
//!   project.
//! - The families that scored come best first: by score, then by the number of
//!   distinct signals that matched, then by family id. The first one is the
//!   primary family when its score reaches `min_score`, else the procedure is
//!   [`UNCLASSIFIED`]. The secondary families are the others that score at
//!   least `secondary_ratio` times as much.
//! - A score carries one evidence item for each hit it counts. A hit in the
//!   procedure is cited at its own line, any other hit at the declaration of
//!   the procedure, so that every score cites a line inside the procedure
//!   (ACCEPTANCE A4). When the cap cuts the hits of a signal of module or
//!   project scope, the hits in the procedure are the ones kept.
//! - The families of the file are the sums of the scores of its procedures,
//!   ranked the same way, with no evidence. The code outside the procedures of
//!   a module that has none (a snippet, or a module of `Declare` statements) is
//!   scored like the body of one, for the file only, by the signals of
//!   procedure and module scope: it is still a file about something. The
//!   signals of project scope stay out of it: they describe the project, and
//!   the sheets of a workbook are modules with no code (ADR-0004).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Range, RangeInclusive};

use super::eval::Evaluator;
use super::hit::{Hit, Place};
use super::input::{ProcedureInput, ProjectInput};
use crate::report::{Evidence, FamilyScore};
use crate::rules::{Families, FamilySignal, Scope, Scoring};

#[cfg(test)]
mod tests;

/// The `primary_family` of a procedure or a file that no family claims.
pub const UNCLASSIFIED: &str = "unclassified";

/// The family fields of every procedure of a project, and of the file.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFamilies {
    /// `modules[m][p]` is the procedure `p` of the module `m` of the
    /// [`ProjectInput`]: a module with no procedure has an empty list.
    pub modules: Vec<Vec<ProcedureFamilies>>,
    /// The file.
    pub file: FileFamilies,
}

/// The `primary_family`, `secondary_families` and `families` of a procedure.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureFamilies {
    /// The family that the procedure is about, or [`UNCLASSIFIED`].
    pub primary_family: String,
    /// The other families that score close to it, best first. Empty for an
    /// unclassified procedure.
    pub secondary_families: Vec<String>,
    /// The scores, best first, each with the evidence of the hits it counts. A
    /// family with no counted hit is not listed, and neither is `unclassified`.
    pub families: Vec<FamilyScore>,
}

/// The `primary_family` and `families` of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileFamilies {
    /// The family that the file is about, or [`UNCLASSIFIED`].
    pub primary_family: String,
    /// The sums of the scores of the procedures and of the code outside them
    /// (see the module documentation), best first, with no evidence.
    pub families: Vec<FamilyScore>,
}

/// Scores the procedures of `project` against `rules`, then the file.
///
/// Pure and deterministic: ties are broken as the module documentation says,
/// and the lists come out in the order of their scores.
pub fn classify(project: &ProjectInput, rules: &Families) -> ProjectFamilies {
    let scorer = Scorer::new(project, rules);
    let mut sums = Tallies::default();
    let mut modules = Vec::with_capacity(project.modules.len());
    for (module, owner) in project.modules.iter().enumerate() {
        let mut procedures = Vec::with_capacity(owner.procedures.len());
        if owner.procedures.is_empty() {
            sums.merge(&scorer.score_code(module));
        } else {
            let module_hits = scorer.module_hits(module);
            for (index, procedure) in owner.procedures.iter().enumerate() {
                let at = Located {
                    module,
                    index,
                    procedure,
                };
                let tallies = scorer.score(at, &module_hits);
                sums.merge(&tallies);
                procedures.push(procedure_families(tallies, &rules.scoring));
            }
        }
        modules.push(procedures);
    }
    ProjectFamilies {
        modules,
        file: file_families(sums, &rules.scoring),
    }
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
            let (hits, inside) = match signal.scope {
                Scope::Procedure => {
                    let (module, index) = (at.module, at.index);
                    own =
                        self.evaluator
                            .procedure_hits(module, index, &signal.matcher, signal.text);
                    (own.as_slice(), 0..own.len())
                }
                Scope::Module => {
                    let hits = hits_at(module_hits, position);
                    (hits, hits_inside(hits, at))
                }
                Scope::Project => {
                    let hits = hits_at(&self.project_hits, position);
                    (hits, hits_inside(hits, at))
                }
            };
            let counted = counted_hits(hits, &inside, max_hits(signal));
            if counted.is_empty() {
                continue;
            }
            let cited = counted.iter().map(|hit| evidence(&signal.id, hit, at));
            tallies
                .count(family, signal, counted.len())
                .evidence
                .extend(cited);
        }
        tallies
    }

    /// The scores of the code of a module that has no procedure. Its
    /// declarations stand for the body of one, so the signals of procedure and
    /// module scope read them. The scores count for the file only, and have no
    /// evidence. The signals of project scope are left out: they describe the
    /// project, and the sheets of a workbook would each add them again.
    fn score_code(&self, module: usize) -> Tallies<'a> {
        let mut tallies = Tallies::default();
        for &(family, signal) in &self.signals {
            if signal.scope == Scope::Project {
                continue;
            }
            let hits = self
                .evaluator
                .module_hits(module, &signal.matcher, signal.text);
            let counted = hits.len().min(max_hits(signal));
            if counted > 0 {
                tallies.count(family, signal, counted);
            }
        }
        tallies
    }
}

/// The most hits that the signal counts.
fn max_hits(signal: &FamilySignal) -> usize {
    usize::try_from(signal.max_hits_per_procedure).unwrap_or(usize::MAX)
}

/// What one hit of the signal adds to a score, in half points: twice its
/// weight when the signal reads the procedure only, its weight (half of the
/// weight in points) when it reaches beyond.
fn half_points_per_hit(signal: &FamilySignal) -> u64 {
    let weight = u64::from(signal.weight);
    match signal.scope {
        Scope::Procedure => weight.saturating_mul(2),
        Scope::Module | Scope::Project => weight,
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
    /// Adds the points and the signals of `other`: all that a file keeps of a
    /// procedure.
    fn merge(&mut self, other: &Tally<'a>) {
        self.half_points = self.half_points.saturating_add(other.half_points);
        self.signals.extend(&other.signals);
    }

    /// The score of the report. An `f64` holds every whole number below 2^53
    /// exactly, far above the half points of any rules.
    fn score(&self) -> f64 {
        self.half_points as f64 / 2.0
    }

    /// The score of `family`, with its evidence in the order of the lines.
    fn into_score(mut self, family: &str) -> FamilyScore {
        let by_line = |a: &Evidence, b: &Evidence| (a.line, &a.rule_id).cmp(&(b.line, &b.rule_id));
        self.evidence.sort_by(by_line);
        FamilyScore {
            family: family.to_owned(),
            score: self.score(),
            evidence: Some(self.evidence),
        }
    }
}

/// The tallies of a procedure or of a file, by family id.
#[derive(Debug, Default)]
struct Tallies<'a>(BTreeMap<&'a str, Tally<'a>>);

impl<'a> Tallies<'a> {
    /// Counts `hits` hits of `signal` for `family`, and gives the tally of the
    /// family for the caller to add the evidence to.
    fn count(&mut self, family: &'a str, signal: &'a FamilySignal, hits: usize) -> &mut Tally<'a> {
        let hits = u64::try_from(hits).unwrap_or(u64::MAX);
        let added = half_points_per_hit(signal).saturating_mul(hits);
        let tally = self.0.entry(family).or_default();
        tally.half_points = tally.half_points.saturating_add(added);
        tally.signals.insert(&signal.id);
        tally
    }

    /// Adds the tallies of `other`: the file sums those of its procedures.
    fn merge(&mut self, other: &Tallies<'a>) {
        for (&family, tally) in &other.0 {
            self.0.entry(family).or_default().merge(tally);
        }
    }

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

/// The primary family of a ranking, and the secondary ones. Below `min_score`
/// nothing is primary, and then nothing is secondary either.
fn verdict(ranked: &[(&str, Tally<'_>)], scoring: &Scoring) -> (String, Vec<String>) {
    let unclassified = || (UNCLASSIFIED.to_owned(), Vec::new());
    let Some(((family, best), others)) = ranked.split_first() else {
        return unclassified();
    };
    let top = best.score();
    if top >= scoring.min_score {
        let floor = scoring.secondary_ratio * top;
        let close = others.iter().filter(|(_, tally)| tally.score() >= floor);
        let secondary = close.map(|&(id, _)| id.to_owned()).collect();
        ((*family).to_owned(), secondary)
    } else {
        unclassified()
    }
}

/// The family fields of a procedure, from its tallies.
fn procedure_families(tallies: Tallies<'_>, scoring: &Scoring) -> ProcedureFamilies {
    let ranked = tallies.ranked();
    let (primary_family, secondary_families) = verdict(&ranked, scoring);
    let families = ranked
        .into_iter()
        .map(|(family, tally)| tally.into_score(family))
        .collect();
    ProcedureFamilies {
        primary_family,
        secondary_families,
        families,
    }
}

/// The family fields of the file, from the sums of its tallies. The file has no
/// secondary families, and no evidence to give.
fn file_families(sums: Tallies<'_>, scoring: &Scoring) -> FileFamilies {
    let ranked = sums.ranked();
    let (primary_family, _) = verdict(&ranked, scoring);
    let families = ranked
        .into_iter()
        .map(|(family, tally)| FamilyScore {
            family: family.to_owned(),
            score: tally.score(),
            evidence: None,
        })
        .collect();
    FileFamilies {
        primary_family,
        families,
    }
}
