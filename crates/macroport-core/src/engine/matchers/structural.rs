// SPDX-License-Identifier: Apache-2.0
//! The structural predicates of SPEC section 7, the closed list of Lot 1.
//!
//! The engine decides `auto_exec` and `event_handler` itself, from the name of a
//! procedure and the type of its module. Both are about a procedure as a whole:
//! the hit cites its declaration.
//!
//! A list of predicates matches where any of them holds. A procedure that
//! several of them hold for gives one hit.

use super::declaration_hit;
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

/// `structural`: the procedures for which a predicate of the list holds. Only
/// `auto_exec` and `event_handler` give hits yet; the six other predicates give
/// none.
pub(in crate::engine) fn structural<'p>(
    units: &[Unit<'p>],
    predicates: &[StructuralPredicate],
    hits: &mut Vec<Hit<'p>>,
) {
    procedures(units, predicates, hits);
}

/// A hit at the declaration of each procedure for which a predicate holds. The
/// declarations of a module are not a procedure.
fn procedures<'p>(units: &[Unit<'p>], predicates: &[StructuralPredicate], hits: &mut Vec<Hit<'p>>) {
    for unit in units {
        if let Some(procedure) = unit.procedure
            && predicates
                .iter()
                .any(|&predicate| holds(predicate, unit.owner, procedure))
        {
            hits.push(declaration_hit(unit.module, procedure));
        }
    }
}

/// Whether `predicate` holds for the whole of `procedure`, which is in the
/// module `owner`.
fn holds(predicate: StructuralPredicate, owner: &ModuleInput, procedure: &ProcedureInput) -> bool {
    match predicate {
        StructuralPredicate::AutoExec => is_auto_exec(owner.module_type, &procedure.name),
        StructuralPredicate::EventHandler => is_event_handler(
            owner.module_type,
            &procedure.name,
            &owner.with_events,
            &owner.controls,
        ),
        // The name and the module type do not decide these.
        StructuralPredicate::UdfCandidate
        | StructuralPredicate::ChrObfuscation
        | StructuralPredicate::LongHexOrBase64Literal
        | StructuralPredicate::NestedLoopCompare
        | StructuralPredicate::PossiblyUnused
        | StructuralPredicate::ParseRecovered => false,
    }
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
