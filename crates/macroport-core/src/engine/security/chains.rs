// SPDX-License-Identifier: Apache-2.0
//! The chains of a file (SPEC M3): combined signals that hold when the signals
//! they name are raised in the file.

use std::collections::BTreeMap;

use super::FileSignals;
use crate::report::{Evidence, Signal};
use crate::rules::{ChainTerm, SecurityChain};

/// The line of the first evidence of each signal that the file raises, by
/// signal id. A signal that cites no line, such as one raised by a reference,
/// has none. When the file raises a signal in several places, the first one in
/// the order of the report gives the line.
type RaisedLines<'a> = BTreeMap<&'a str, Option<u32>>;

/// The chains that hold in a file whose signals are `found`, in the order of
/// the rules.
///
/// A chain holds when each of its terms does. A term is a signal that the file
/// raises, or a list of terms of which one holds. The signal of a chain cites
/// each signal that makes it hold, once, in the order of the terms and with the
/// first line of that signal, and quotes no snippet. It has no module and no
/// procedure, as it belongs to the whole file.
pub(super) fn chain_signals(chains: &[SecurityChain], found: &FileSignals) -> Vec<Signal> {
    let mut raised = RaisedLines::new();
    for signal in found.signals() {
        let first = signal.evidence.iter().flatten().next();
        let line = first.map(|first| first.line);
        raised.entry(&signal.signal_id).or_insert(line);
    }
    (chains.iter())
        .filter_map(|chain| raise_chain(chain, &raised))
        .collect()
}

/// The signal of `chain`, none when it does not hold.
fn raise_chain(chain: &SecurityChain, raised: &RaisedLines<'_>) -> Option<Signal> {
    let held: Option<Vec<Vec<&str>>> = (chain.all.iter())
        .map(|term| holding(term, raised))
        .collect();
    let mut cited: Vec<&str> = Vec::new();
    for id in held?.into_iter().flatten() {
        if !cited.contains(&id) {
            cited.push(id);
        }
    }
    let evidence: Vec<Evidence> = cited
        .into_iter()
        .filter_map(|id| {
            let line = raised.get(id).copied().flatten()?;
            Some(Evidence {
                rule_id: id.to_owned(),
                line,
                snippet: None,
            })
        })
        .collect();
    Some(Signal {
        signal_id: chain.id.clone(),
        severity: chain.severity,
        module: None,
        procedure: None,
        evidence: (!evidence.is_empty()).then_some(evidence),
    })
}

/// The signals that make `term` hold when the file raises those of `raised`, in
/// the order of the term, or none when it does not hold. A list of terms is read
/// to its end, so that every term of it that holds is named, not only the
/// first.
fn holding<'r>(term: &'r ChainTerm, raised: &RaisedLines<'_>) -> Option<Vec<&'r str>> {
    match term {
        ChainTerm::Signal(id) => raised.contains_key(id.as_str()).then(|| vec![id.as_str()]),
        ChainTerm::Any(terms) => {
            let members: Vec<&str> = (terms.iter())
                .filter_map(|member| holding(member, raised))
                .flatten()
                .collect();
            (!members.is_empty()).then_some(members)
        }
    }
}
