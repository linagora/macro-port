// SPDX-License-Identifier: Apache-2.0
//! The structural predicates of SPEC section 7, the closed list of Lot 1.
//!
//! The engine decides `auto_exec`, `event_handler` and
//! `long_hex_or_base64_literal` itself, from names, module types and string
//! literals. The five others need the parse structure: the parser decides them
//! (`ParserPredicates`) and the engine turns what it decided into hits.
//! `parse_recovered` is also decided for a module as a whole
//! (`ModuleInput::parse_recovered`), since the parser may recover outside any
//! procedure.
//!
//! A list of predicates matches where any of them holds. An item that several
//! of them hold for, be it a procedure, a statement or a literal, gives one
//! hit.

use std::collections::BTreeSet;

use super::{hit_at, per_item, per_procedure, statement_hit};
use crate::engine::hit::Hit;
use crate::engine::input::{ModuleInput, NameSet, ProcedureInput};
use crate::engine::names::{fold_case, is_one_of, same_name, without_type_char};
use crate::engine::unit::Unit;
use crate::rules::{ModuleType, StructuralPredicate};

/// Names of the procedures that Excel runs by themselves when a workbook opens
/// or closes. They count in whatever module they are.
const AUTO_MACROS: [&str; 5] = [
    "Auto_Open",
    "Auto_Close",
    "AutoOpen",
    "AutoClose",
    "AutoExec",
];

/// Events of the workbook that run when it opens, becomes active or closes. A
/// procedure of this name runs by itself in a document module only.
const WORKBOOK_AUTO_EVENTS: [&str; 3] =
    ["Workbook_Open", "Workbook_Activate", "Workbook_BeforeClose"];

/// The objects whose events a document module handles without declaring them.
const DOCUMENT_OBJECTS: [&str; 3] = ["Workbook", "Worksheet", "Chart"];

/// The object whose events a form module handles without declaring it.
const FORM_OBJECT: &str = "UserForm";

/// The longest name, in characters, that VBA accepts. A procedure with a longer
/// name is none that Excel runs, and a hostile file may give it a name of
/// megabytes with an underscore at every other character, each one an object
/// to look up.
const MAX_NAME_CHARS: usize = 255;

/// The length from which a string literal in the hex or base64 alphabet is
/// long (SPEC section 7).
const LONG_LITERAL_CHARS: usize = 200;

/// `structural`: the procedures, statements and literals for which a predicate
/// of the list holds. A predicate about a procedure gives a hit at its
/// declaration, one about statements a hit at each statement that the parser
/// cites, and `long_hex_or_base64_literal` a hit for each literal it finds.
/// `parse_recovered` gives a hit for each module that the parser recovered in as
/// well.
pub(in crate::engine) fn structural<'p>(
    units: &[Unit<'p>],
    predicates: &[StructuralPredicate],
    hits: &mut Vec<Hit<'p>>,
) {
    procedures(units, predicates, hits);
    recovered_modules(units, predicates, hits);
    statements(units, predicates, hits);
    if predicates.contains(&StructuralPredicate::LongHexOrBase64Literal) {
        let fits = |literal: &String| is_long_encoded(literal);
        per_item(units, |line| line.strings.as_slice(), fits, hits);
    }
}

/// A hit at the declaration of each procedure for which a predicate holds. The
/// declarations of a module are not a procedure.
fn procedures<'p>(units: &[Unit<'p>], predicates: &[StructuralPredicate], hits: &mut Vec<Hit<'p>>) {
    let fits = |owner: &ModuleInput, procedure: &ProcedureInput| {
        predicates
            .iter()
            .any(|&predicate| holds(predicate, owner, procedure))
    };
    per_procedure(units, fits, hits);
}

/// A hit for each module that the parser recovered in, when `parse_recovered`
/// is in the list. The recovery may be outside any procedure, as a stray
/// `End Sub` is, so the module carries it and its hit is that of `module_type`:
/// the first line, quoting the module name. The declarations of a module stand
/// for it, so the unit of a procedure alone has no such hit.
fn recovered_modules<'p>(
    units: &[Unit<'p>],
    predicates: &[StructuralPredicate],
    hits: &mut Vec<Hit<'p>>,
) {
    if !predicates.contains(&StructuralPredicate::ParseRecovered) {
        return;
    }
    for unit in units {
        if unit.procedure.is_none() && unit.owner.parse_recovered {
            hits.push(hit_at(unit.module, 1, &unit.owner.name));
        }
    }
}

/// Whether `predicate` holds for the whole of `procedure`, which is in the
/// module `owner`.
fn holds(predicate: StructuralPredicate, owner: &ModuleInput, procedure: &ProcedureInput) -> bool {
    let parsed = &procedure.predicates;
    match predicate {
        StructuralPredicate::AutoExec => is_auto_exec(owner.module_type, &procedure.name),
        StructuralPredicate::EventHandler => is_event_handler(
            owner.module_type,
            &procedure.name,
            &owner.with_events,
            &owner.controls,
        ),
        StructuralPredicate::UdfCandidate => parsed.udf_candidate,
        StructuralPredicate::PossiblyUnused => parsed.possibly_unused,
        StructuralPredicate::ParseRecovered => parsed.parse_recovered,
        // These hold for statements or for literals, not for a procedure.
        StructuralPredicate::ChrObfuscation
        | StructuralPredicate::NestedLoopCompare
        | StructuralPredicate::LongHexOrBase64Literal => false,
    }
}

/// A hit at each statement that the parser cites for a predicate of the list,
/// by its position among the lines of the procedure: every statement for
/// `chr_obfuscation`, the last comparison only for `nested_loop_compare`. It
/// cites the line of the statement and quotes it. A position outside the lines
/// cites nothing.
fn statements<'p>(units: &[Unit<'p>], predicates: &[StructuralPredicate], hits: &mut Vec<Hit<'p>>) {
    for unit in units {
        let Some(procedure) = unit.procedure else {
            continue;
        };
        let parsed = &procedure.predicates;
        // The positions that several predicates cite are cited once.
        let mut cited: BTreeSet<usize> = BTreeSet::new();
        for predicate in predicates {
            match predicate {
                StructuralPredicate::ChrObfuscation => cited.extend(&parsed.chr_obfuscation),
                StructuralPredicate::NestedLoopCompare => cited.extend(last_comparison(procedure)),
                StructuralPredicate::UdfCandidate
                | StructuralPredicate::EventHandler
                | StructuralPredicate::AutoExec
                | StructuralPredicate::LongHexOrBase64Literal
                | StructuralPredicate::PossiblyUnused
                | StructuralPredicate::ParseRecovered => {}
            }
        }
        for position in cited {
            if let Some(statement) = procedure.lines.get(position) {
                hits.push(statement_hit(unit.module, statement));
            }
        }
    }
}

/// The position of the last comparison in a nested loop that the parser cited
/// among the lines of `procedure`, if any. `nested_loop_compare` gives this one
/// hit for the procedure, however many comparisons it holds: the weights of the
/// rules were calibrated on one hit, and each hit counts up to
/// `max_hits_per_procedure` times.
fn last_comparison(procedure: &ProcedureInput) -> Option<usize> {
    let lines = procedure.lines.len();
    let cited = procedure.predicates.nested_loop_compare.iter().copied();
    cited.filter(|&position| position < lines).max()
}

/// `auto_exec`: whether a procedure of this name, in a module of this type,
/// runs by itself when the workbook opens or closes. Names compare whatever
/// their case and type character.
///
/// The auto macros count in any module: the sample 14-01 has an `Auto_Open` in a
/// standard module. The events of the workbook count in a document module only
/// (SPEC section 7): a standard module that has a `Workbook_Open` has an
/// ordinary procedure of that name.
pub fn is_auto_exec(module_type: ModuleType, name: &str) -> bool {
    is_one_of(name, &AUTO_MACROS)
        || (module_type == ModuleType::Document && is_one_of(name, &WORKBOOK_AUTO_EVENTS))
}

/// `event_handler`: whether a procedure of this name, in a module of this type,
/// is named `<object>_<event>` for an object whose events the module handles.
/// `with_events` and `controls` are those of the module.
///
/// The object and the event may both hold underscores: `txt_Name_Change` is
/// the event `Change` of the control `txt_Name`, and `Worksheet_Calc_Done` is
/// still a `Worksheet_*` name of SPEC section 7. So each underscore is tried as
/// the end of the object. The event is not empty. A name longer than VBA allows
/// is no handler.
pub fn is_event_handler(
    module_type: ModuleType,
    name: &str,
    with_events: &NameSet,
    controls: &NameSet,
) -> bool {
    let name = without_type_char(name);
    if name.chars().nth(MAX_NAME_CHARS).is_some() {
        return false;
    }
    // `key` holds the name up to the underscore being tried, in the form in
    // which names are compared: it is the key of the object, built once for all
    // the underscores.
    let mut key = String::new();
    for (offset, c) in name.char_indices() {
        if c == '_' {
            let object = name.get(..offset).unwrap_or_default();
            let rest = name.get(offset..).unwrap_or_default();
            let event = rest.strip_prefix('_').unwrap_or_default();
            if !event.is_empty()
                && handles_events_of(module_type, object, &key, with_events, controls)
            {
                return true;
            }
        }
        key.extend(fold_case(c));
    }
    false
}

/// Whether a module of this type handles the events of `object`, whose key in
/// the form in which names are compared is `key`. A document module handles
/// those of the workbook, the worksheets and the charts without a declaration,
/// and a form those of itself. Beyond that, a class or a document module
/// handles the `WithEvents` variables that it declares, and a form its
/// controls. A standard module has no events.
fn handles_events_of(
    module_type: ModuleType,
    object: &str,
    key: &str,
    with_events: &NameSet,
    controls: &NameSet,
) -> bool {
    match module_type {
        ModuleType::Standard => false,
        ModuleType::Class => with_events.contains_key(key),
        ModuleType::Document => {
            is_one_of(object, &DOCUMENT_OBJECTS) || with_events.contains_key(key)
        }
        ModuleType::Form => same_name(object, FORM_OBJECT) || controls.contains_key(key),
    }
}

/// `long_hex_or_base64_literal`: whether `literal` is at least 200 characters
/// long and made of the base64 alphabet only: letters, digits, `+` and `/`,
/// then at most two `=` of padding. The hex digits are part of it. Any other
/// character, a `=` that is not at the end included, rules the literal out.
fn is_long_encoded(literal: &str) -> bool {
    let in_alphabet = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '/');
    // A literal that passes is ASCII, so its length in bytes is its length in
    // characters. The short ones, which are the most, are not read.
    literal.len() >= LONG_LITERAL_CHARS
        && matches!(literal.trim_start_matches(in_alphabet), "" | "=" | "==")
}
