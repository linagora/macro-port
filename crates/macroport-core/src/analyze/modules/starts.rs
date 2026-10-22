// SPDX-License-Identifier: Apache-2.0
//! The procedures that the host starts by itself, and what the call graph says of
//! them (ADR-0012).
//!
//! The graph cannot tell which procedures the host starts. The event handlers and
//! the auto-exec procedures are decided by the structural predicates of the rule
//! engine (SPEC section 7), and the core keeps no copy of them in `vba/`, so
//! [`Starts`] asks the engine about each procedure of a module, and the front end
//! tells the graph. What the graph answers goes to the report ([`put_links`]) and
//! back to the engine, which reads `possibly_unused` as a predicate
//! ([`mark_possibly_unused`]).
//!
//! There is one thing that the engine cannot decide, and the report then says so.
//! The controls of a form are those of its designer, which nothing reads yet (M1
//! reads no form designer, and a text `.frm` is read without its `Begin` blocks),
//! and a sheet may hold controls too. A procedure of such a module that is named
//! `<object>_<event>` may handle the event of a control, which the host starts,
//! and the engine takes it for no handler: it is [`Start::Unknown`]. The report
//! leaves out whether it is an entry point and whether it is an event handler. It
//! leaves out `possibly_unused` too, unless something calls the procedure, since
//! a call is there whoever starts it, and the lack of one is not enough. The
//! calls are known all the same.

use crate::engine::{ModuleInput, is_auto_exec, is_event_handler};
use crate::report::ModuleReport;
use crate::rules::ModuleType;
use crate::vba::ProcedureLinks;

/// Whether the host starts a procedure by itself, as the rule engine decides it
/// from the name of the procedure and the type of its module (SPEC section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Start {
    /// An event handler, as the predicate `event_handler` says: an event of the
    /// workbook, of a worksheet or of a chart in a document module, of a
    /// `WithEvents` variable in a class or a document module, or of the form or
    /// of one of its known controls in a form module.
    EventHandler,
    /// An auto-exec procedure that is no event handler, such as the `Auto_Open`
    /// of any module. The host starts it too. (`Workbook_Open` in a document
    /// module is both, and is an `EventHandler` here.)
    AutoExec,
    /// Neither: only a call, a button or a shortcut starts it.
    Neither,
    /// Nothing can tell, since the procedure may handle an event of a control
    /// that no stage read ([`may_handle_a_control`]).
    Unknown,
}

impl Start {
    /// Whether the host starts the procedure: what the call graph is told.
    pub(super) fn is_started(self) -> bool {
        matches!(self, Self::EventHandler | Self::AutoExec)
    }

    /// The field `is_event_handler` of the report: the engine's answer, which is
    /// true for the predicate `event_handler` only, and none when nothing can
    /// tell.
    pub(super) fn is_event_handler(self) -> Option<bool> {
        match self {
            Self::EventHandler => Some(true),
            Self::AutoExec | Self::Neither => Some(false),
            Self::Unknown => None,
        }
    }
}

/// What the host starts among the procedures of a module: the [`Start`] of each,
/// in the order of the module. Empty for a module that was not read.
#[derive(Debug, Default)]
pub(super) struct Starts(Vec<Start>);

impl Starts {
    /// What the host starts among the procedures of `module`. The engine takes a
    /// procedure for an event handler of a control only when it knows the
    /// controls of the form ([`ModuleInput::controls`]), which nothing fills
    /// today, so a procedure that could be one is [`Start::Unknown`] instead of
    /// the `false` that the engine gives.
    pub(super) fn of(module: &ModuleInput) -> Self {
        let kind = module.module_type;
        let starts = (module.procedures.iter())
            .map(|procedure| {
                let name = procedure.name.as_str();
                if is_event_handler(kind, name, &module.with_events, &module.controls) {
                    Start::EventHandler
                } else if is_auto_exec(kind, name) {
                    Start::AutoExec
                } else if may_handle_a_control(module, name) {
                    Start::Unknown
                } else {
                    Start::Neither
                }
            })
            .collect();
        Self(starts)
    }

    /// The start of the procedure at `index`. A module has no procedure past its
    /// last, and one that is asked for anyway is started by a call and nothing
    /// else.
    pub(super) fn at(&self, index: usize) -> Start {
        self.0.get(index).copied().unwrap_or(Start::Neither)
    }

    /// For each procedure, whether the host starts it: the `started` argument of
    /// `CallGraph::add_module`.
    pub(super) fn started(&self) -> Vec<bool> {
        self.0.iter().map(|start| start.is_started()).collect()
    }
}

/// Whether a procedure called `name` of `module` may handle an event of a
/// control that no stage read, though the engine does not take it for a handler.
///
/// The controls of a form are those of its designer, and the handlers of their
/// events are named `<object>_<event>`, as `Button1_Click` is. So are those of
/// the ActiveX controls of a sheet, which the module of the sheet holds, and which
/// the engine does not take for handlers even when it knows the controls (SPEC
/// section 7 gives a document module the events of the workbook, the worksheets
/// and the charts, and those of its `WithEvents` variables). A procedure of such
/// a module that has that name may then be one, and the engine, which has no
/// control to match it with, takes it for none. A module whose controls are known
/// is not in this case: a name that is none of them is no handler.
fn may_handle_a_control(module: &ModuleInput, name: &str) -> bool {
    let holds_controls = matches!(module.module_type, ModuleType::Form | ModuleType::Document);
    // An underscore with a name before it and one after it.
    let is_named_like_an_event =
        (name.find('_')).is_some_and(|at| at > 0 && at.saturating_add(1) < name.len());
    holds_controls && module.controls.is_empty() && is_named_like_an_event
}

/// What the report says of a procedure that the call graph answered for: whether
/// it is an entry point and whether it is possibly unused. `None` is what nothing
/// can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Judged {
    is_entry_point: Option<bool>,
    possibly_unused: Option<bool>,
}

impl Judged {
    /// What `links`, the answer of the graph for a procedure, and `start` give.
    /// The graph has no `possibly_unused` when it judged nothing, and then the
    /// report has none. A procedure that may handle the event of a control is
    /// told for neither field but for the one value that does not depend on who
    /// starts it: it is not unused when something calls it ([`Start::Unknown`]).
    fn of(links: &ProcedureLinks, start: Start) -> Self {
        match start {
            Start::Unknown => Self {
                is_entry_point: None,
                possibly_unused: links.possibly_unused.filter(|unused| !unused),
            },
            Start::EventHandler | Start::AutoExec | Start::Neither => Self {
                is_entry_point: Some(links.is_entry_point),
                possibly_unused: links.possibly_unused,
            },
        }
    }
}

/// Puts what the call graph found into the procedures of `modules`: whether each
/// is an entry point and possibly unused, and the procedures it calls. `links`
/// has the modules of the file in their order, and for each of them its
/// procedures in theirs, as `starts` does. A procedure that the graph has no
/// answer for, one of a module that it did not take or that is out of its
/// bounds, keeps `None` for what it could not tell, and so does one that may
/// handle an event of a control that was not read ([`Judged::of`]).
pub(super) fn put_links(
    modules: &mut [ModuleReport],
    links: Vec<Vec<ProcedureLinks>>,
    starts: &[Starts],
) {
    for ((module, links), starts) in modules.iter_mut().zip(links).zip(starts) {
        for (index, (procedure, links)) in module.procedures.iter_mut().zip(links).enumerate() {
            let judged = Judged::of(&links, starts.at(index));
            procedure.is_entry_point = judged.is_entry_point;
            procedure.possibly_unused = judged.possibly_unused;
            procedure.calls = links.calls;
        }
    }
}

/// Gives the procedures of the input of the rule engine the structural predicate
/// `possibly_unused` that the call graph found. The predicate is false for a
/// procedure that the report leaves `possibly_unused` out of, since the engine
/// cannot tell a value that is missing from a false one. `links`, `starts` and
/// `modules` have the modules of the file in the same order.
pub(super) fn mark_possibly_unused(
    modules: &mut [ModuleInput],
    links: &[Vec<ProcedureLinks>],
    starts: &[Starts],
) {
    for ((module, links), starts) in modules.iter_mut().zip(links).zip(starts) {
        for (index, (procedure, links)) in module.procedures.iter_mut().zip(links).enumerate() {
            let judged = Judged::of(links, starts.at(index));
            procedure.predicates.possibly_unused = judged.possibly_unused.unwrap_or(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Start::{AutoExec, EventHandler, Neither, Unknown};
    use super::*;
    use crate::analyze::input::{Room, adapt};
    use crate::analyze::modules::Analysis;

    /// What the engine reads of a module of this type made of `text`, with the
    /// controls `controls` (none are read today).
    fn input_of(module_type: ModuleType, text: &str, controls: &[&str]) -> ModuleInput {
        let analysis = Analysis::of(text).expect("a short module");
        let adapted = adapt("Module1", module_type, &analysis, &mut Room::of_a_file());
        let mut input = adapted.expect("a module that fits the room").input;
        input.controls = controls.iter().collect();
        input
    }

    fn links(is_entry_point: bool, possibly_unused: Option<bool>) -> ProcedureLinks {
        ProcedureLinks {
            is_entry_point,
            possibly_unused,
            calls: None,
        }
    }

    #[test]
    fn the_host_starts_the_event_handlers_and_the_auto_exec_procedures() {
        let starts = |module_type| {
            let text = "Private WithEvents App As Application\n\
                        Sub Auto_Open()\nEnd Sub\n\
                        Sub App_NewWorkbook()\nEnd Sub\n\
                        Sub Workbook_Open()\nEnd Sub\n\
                        Sub Helper()\nEnd Sub\n";
            Starts::of(&input_of(module_type, text, &[]))
        };

        let standard = starts(ModuleType::Standard);
        let document = starts(ModuleType::Document);
        let class = starts(ModuleType::Class);

        // A standard module has no events: its auto-exec procedure is started by
        // the host and is no event handler.
        assert_eq!(standard.0, [AutoExec, Neither, Neither, Neither]);
        // A document module handles those of its workbook and of its variables.
        // `App_NewWorkbook` is named after the variable, and `Helper` has no
        // underscore: neither can be the handler of a control.
        assert_eq!(document.0, [AutoExec, EventHandler, EventHandler, Neither]);
        // A class handles those of its variables only. The auto macros count in
        // any module, as the rule engine's `auto_exec` does: the two agree.
        assert_eq!(class.0, [AutoExec, EventHandler, Neither, Neither]);
        assert_eq!(class.started(), [true, true, false, false]);
        // A module has no procedure past its last.
        assert_eq!(class.at(4), Neither);
        assert_eq!(Starts::default().at(0), Neither);
    }

    #[test]
    fn the_report_tells_an_event_handler_by_the_engines_predicate_only() {
        let all = [EventHandler, AutoExec, Neither, Unknown];

        // The graph is told of the event handlers and of the auto-exec procedures,
        // which are no event handlers. It is told of no procedure that a call
        // starts, and of none that may handle the event of a control.
        assert_eq!(all.map(Start::is_started), [true, true, false, false]);
        assert_eq!(
            all.map(Start::is_event_handler),
            [Some(true), Some(false), Some(false), None]
        );
    }

    #[test]
    fn nothing_can_tell_if_a_procedure_handles_the_event_of_a_control_that_nobody_read() {
        const CONTROLS: &str = "Sub UserForm_Initialize()\nEnd Sub\n\
                                Sub Button1_Click()\nEnd Sub\n\
                                Sub Helper()\nEnd Sub\n\
                                Sub Do_Work()\nEnd Sub\n\
                                Sub Auto_Open()\nEnd Sub\n\
                                Sub Trailing_()\nEnd Sub\n";
        let starts =
            |module_type, controls: &[&str]| Starts::of(&input_of(module_type, CONTROLS, controls));

        let form = starts(ModuleType::Form, &[]);
        let known = starts(ModuleType::Form, &["Button1"]);
        let document = starts(ModuleType::Document, &[]);
        let standard = starts(ModuleType::Standard, &[]);
        let class = starts(ModuleType::Class, &[]);

        // The form handles the events of `UserForm`. `Button1_Click` and
        // `Do_Work` have the form of a handler of a control and the engine knows
        // no control: nothing can tell. `Helper` has no underscore, `Trailing_`
        // no event, and `Auto_Open` is started by the host whatever it is.
        assert_eq!(
            form.0,
            [EventHandler, Unknown, Neither, Unknown, AutoExec, Neither]
        );
        assert_eq!(form.started(), [true, false, false, false, true, false]);
        // With its controls, the form is known: a name that none of them has is
        // no handler, and `Do_Work` is not.
        assert_eq!(
            known.0,
            [
                EventHandler,
                EventHandler,
                Neither,
                Neither,
                AutoExec,
                Neither
            ]
        );
        // A sheet may hold controls too, and `UserForm_Initialize` is none of
        // the events of a workbook, a worksheet or a chart.
        assert_eq!(
            document.0,
            [Unknown, Unknown, Neither, Unknown, AutoExec, Neither]
        );
        // The other modules hold none.
        let others = [Neither, Neither, Neither, Neither, AutoExec, Neither];
        assert_eq!(standard.0, others);
        assert_eq!(class.0, others);
    }

    #[test]
    fn a_procedure_that_may_handle_a_control_is_judged_as_far_as_a_call_shows() {
        let judged = |start, links: ProcedureLinks| {
            let judged = Judged::of(&links, start);
            (judged.is_entry_point, judged.possibly_unused)
        };

        // The answer of the graph is the report's for what the engine decided.
        assert_eq!(
            judged(Neither, links(false, Some(true))),
            (Some(false), Some(true))
        );
        assert_eq!(
            judged(EventHandler, links(true, Some(false))),
            (Some(true), Some(false))
        );
        assert_eq!(judged(AutoExec, links(true, None)), (Some(true), None));
        // For a procedure that may handle the event of a control, nothing says
        // that the host starts it or not. A call shows that it is used, and the
        // lack of one that it may be, or not.
        assert_eq!(
            judged(Unknown, links(false, Some(false))),
            (None, Some(false))
        );
        assert_eq!(judged(Unknown, links(false, Some(true))), (None, None));
        assert_eq!(judged(Unknown, links(false, None)), (None, None));
    }
}
