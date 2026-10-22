// SPDX-License-Identifier: Apache-2.0
//! The call graph of the project and its entry points (SPEC M2 item 5): which
//! procedure calls which, by name and as well as the text tells, and which
//! procedures nothing calls.
//!
//! The graph is fed a module at a time ([`CallGraph::add_module`]) and a
//! statement at a time ([`CallGraph::read`]), the way the front end reads a
//! project: the facts of a statement weigh a few dozen times its text, so the
//! caller keeps none. It answers once the project has been read
//! ([`CallGraph::resolve`]), since a module may call a procedure of one that
//! comes later.
//!
//! # What a name stands for
//!
//! Names compare in any letter case. The procedures of a module that share a
//! name are one *group*, as the `Get`, `Let` and `Set` of a property are: a
//! reference to the name is a reference to the group, and a procedure that
//! refers to its own group refers to nothing (recursion is neither a call nor
//! a use), though the same name after a dot stands for the groups of the other
//! modules too.
//!
//! - A name invoked bare (`Helper x`, `Call Helper`, `y = Helper(1)`, and a
//!   name used as a value, as `Helper` is in `AddressOf Helper`) stands for the
//!   group of that name in the module of the statement, whatever its
//!   visibility, or else for the groups of that name that other modules can
//!   call by that name: public procedures of standard modules.
//! - Any other name that a statement mentions stands for every group of that
//!   name in the project: a name after a dot (`obj.Helper`, `Module1.Helper`,
//!   `Me.Helper`, `ws.Cells(1, 1)`), a name used as a member, and the
//!   identifier in a string that a statement gives to `Run`, `CallByName`,
//!   `OnTime`, `OnKey` or `OnAction` (`"Module1.Helper"` is `Helper`, and so is
//!   the value of a `Const` that the string folded from). Such a name says
//!   nothing of where the procedure is. The name of an argument
//!   (`Pattern:=xlSolid`) names nothing.
//!
//! A name is not told from a variable, a member or a property of the same name.
//! That is the price of reading names and not types, and it errs on the side of
//! a procedure being used. So does the reading of the strings of a statement
//! that starts a procedure by name: all of them are read, those of an
//! expression that does not fold too, since the facts list the literals of such
//! an expression one by one (`Run "Macro" & n` names `Macro`).
//!
//! # Entry points
//!
//! A procedure is an entry point when the caller says the host starts it (an
//! event handler or an auto-exec procedure, which the engine decides; see
//! [`CallGraph::add_module`]), when it is the `Class_Initialize` or
//! `Class_Terminate` of a class, or when it is a public procedure of a standard
//! module that takes no parameter (a button or a shortcut may run it) or that
//! is a function (a worksheet formula may call it). The procedures of a group
//! stand or fall together, for a name is what the host and the other modules
//! have of them: when one is an entry point, all are (the `Let` of a public
//! property of a standard module, which takes a parameter, is one with its
//! `Get`, which takes none). A procedure that is not an entry point and that no
//! other group refers to is *possibly unused*. A procedure that only unused
//! procedures refer to is not.
//!
//! # Limits
//!
//! The graph is built from input that is hostile, so it bounds what it keeps:
//! the names, the statements it reads, the references and the calls it lists.
//! Far above any real project, a limit stops the reading, and the graph then
//! says nothing it is not sure of: [`ProcedureLinks::possibly_unused`] and
//! [`ProcedureLinks::calls`] are `None`, whereas the entry points do not depend
//! on what was read and are always told.

use std::collections::{BTreeMap, BTreeSet};

use super::{Facts, MemberUse, Procedure};
use crate::report::ProcedureKind;
use crate::rules::ModuleType;

/// The longest name, in characters, that VBA accepts. A longer one is no name
/// of a procedure, and a hostile file may give one of megabytes.
const MAX_NAME_CHARS: usize = 255;

/// The longest string, in bytes, that is read for the name of a procedure: the
/// name may be qualified by a workbook and a module.
const MAX_STRING_BYTES: usize = 1024;

/// The calls and the properties that start a procedure from its name, given as
/// a string: `Application.Run`, `CallByName`, `Application.OnTime`,
/// `Application.OnKey` and the `OnAction` of a control. The SPEC names the
/// first two; the others are the same thing for a timer, a shortcut and a
/// button.
const DISPATCHERS: [&str; 5] = ["Run", "CallByName", "OnTime", "OnKey", "OnAction"];

/// The events of a class module: they run when an object of the class is made
/// and when it goes.
const CLASS_EVENTS: [&str; 2] = ["Class_Initialize", "Class_Terminate"];

/// What the graph keeps at most. Real code stays far below each of them: the 80
/// samples of the corpus, read as one project, have some 5,000 distinct names of
/// 50 KB together. On hostile input the limits keep the memory of the graph to
/// about 100 MB, as measured with names of four characters (the shortest, so the
/// most names to the megabyte) that seven procedures all refer to.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// How many bytes the distinct names may take together. A name takes some
    /// 200 bytes of memory besides its own bytes, so this is the limit that
    /// bounds the memory of the graph the most.
    name_bytes: usize,
    /// How many names, calls and strings of statements the graph reads.
    mentions: usize,
    /// How many references it keeps: the pairs of a procedure and a name.
    references: usize,
    /// How many procedures one procedure may list as called.
    calls_per_procedure: usize,
    /// How many procedures all the lists hold together.
    calls: usize,
}

/// The limits of a graph.
const LIMITS: Limits = Limits {
    name_bytes: 1024 * 1024,
    mentions: 4_000_000,
    references: 2_000_000,
    calls_per_procedure: 1000,
    calls: 500_000,
};

/// A module of a [`CallGraph`], as [`CallGraph::add_module`] numbers it. It is
/// the position of the module in the result of [`CallGraph::resolve`], and
/// means nothing for another graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleId(usize);

/// What the graph says of a procedure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureLinks {
    /// The procedure may be started from outside the project's own code (see
    /// the module documentation), or it shares its name with a procedure of its
    /// module that may.
    pub is_entry_point: bool,
    /// The structural predicate `possibly_unused`: the procedure is not an
    /// entry point and no other procedure refers to it. `None` when the graph
    /// could not read the whole project.
    pub possibly_unused: Option<bool>,
    /// The procedures of the project that this one calls, as the project
    /// declares their names (the smallest spelling, in byte order, when
    /// modules spell one differently), each once and sorted in byte order.
    /// Recursion is no call: a procedure that refers to itself adds nothing to
    /// its list, unless the name also stands for a procedure of another module
    /// (see the module documentation). `None` when the graph could not read the
    /// whole project or when the list is too long to keep.
    pub calls: Option<Vec<String>>,
}

/// The call graph of a project, built module by module and statement by
/// statement. Nothing in it is evaluated: names only are read.
#[derive(Debug)]
pub struct CallGraph {
    limits: Limits,
    names: Names,
    groups: Vec<Group>,
    modules: Vec<ModuleInfo>,
    procedures: Vec<ProcedureInfo>,
    /// The references of the procedures that were read, each at least once.
    edges: Vec<Edge>,
    /// The references of the procedure being read, which are not in `edges`
    /// yet: each is kept once there.
    pending: BTreeSet<Edge>,
    /// The procedure being read.
    current: Option<usize>,
    /// How many mentions the graph may still read.
    mentions: usize,
    /// No limit stopped the reading.
    complete: bool,
}

impl Default for CallGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CallGraph {
    /// A graph of no module.
    pub fn new() -> Self {
        Self::with_limits(LIMITS)
    }

    /// A graph of no module that keeps no more than `limits`.
    fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            names: Names::default(),
            groups: Vec::new(),
            modules: Vec::new(),
            procedures: Vec::new(),
            edges: Vec::new(),
            pending: BTreeSet::new(),
            current: None,
            mentions: limits.mentions,
            complete: true,
        }
    }

    /// Adds a module of type `module_type` whose procedures are `procedures`,
    /// before any of its statements is read with [`CallGraph::read`].
    ///
    /// `started[i]` tells that the host starts `procedures[i]` by itself: it is
    /// an event handler or an auto-exec procedure, which the engine decides
    /// (SPEC section 7), or anything else that the caller knows is called from
    /// outside the code, such as the method of an interface that a class
    /// implements. A missing entry means `false`.
    pub fn add_module(
        &mut self,
        module_type: ModuleType,
        procedures: &[Procedure],
        started: &[bool],
    ) -> ModuleId {
        let module = self.modules.len();
        let first = self.procedures.len();
        let mut groups = BTreeMap::new();
        for (position, procedure) in procedures.iter().enumerate() {
            let started = started.get(position).copied().unwrap_or_default();
            let group = self.declare(module_type, procedure, &mut groups);
            let entry = started || is_entry_point(module_type, procedure);
            if let Some(declared) = self.groups.get_mut(group) {
                declared.entry |= entry;
            }
            self.procedures.push(ProcedureInfo {
                module,
                group,
                header: procedure.lines.start,
            });
        }
        self.modules.push(ModuleInfo {
            first,
            count: procedures.len(),
            groups,
        });
        ModuleId(module)
    }

    /// Reads the facts of a statement of a procedure. `procedure` is its
    /// position among the procedures that [`CallGraph::add_module`] was given
    /// for `module`, and `line` the position of the statement among the logical
    /// lines of the module, as [`Procedure::lines`] counts them. The header of
    /// the procedure is left out: the names it holds declare the procedure and
    /// its parameters and refer to nothing.
    ///
    /// The statements of a procedure may come in any order, and those of two
    /// procedures may alternate. The graph keeps a reference once for as long as
    /// a procedure is being read, so each return to a procedure keeps its
    /// references again, against the limit on references: reading one procedure
    /// at a time is the cheaper way. A module or a procedure that the graph
    /// does not know is ignored.
    pub fn read(&mut self, module: ModuleId, procedure: usize, line: usize, facts: &Facts) {
        if !self.complete {
            return;
        }
        let Some(index) = (self.modules.get(module.0)).and_then(|module| module.index(procedure))
        else {
            return;
        };
        let Some(info) = self.procedures.get(index) else {
            return;
        };
        if line == info.header {
            return;
        }
        let caller = Caller {
            index,
            module: info.module,
            group: info.group,
        };
        self.switch_to(index);

        for call in &facts.calls {
            let how = if call.after_dot {
                Mention::Named
            } else {
                Mention::Bare
            };
            self.mention(caller, &call.name, how);
        }
        for member in uninvoked(facts) {
            self.mention(caller, &member.name, Mention::Named);
        }
        if starts_by_name(facts) {
            for name in facts.strings.iter().filter_map(|text| macro_name(text)) {
                self.mention(caller, name, Mention::Named);
            }
        }
    }

    /// Ends the reading and gives what the graph says of each procedure, by
    /// module in the order of [`CallGraph::add_module`] and by procedure in
    /// the order it was given there.
    pub fn resolve(mut self) -> Vec<Vec<ProcedureLinks>> {
        self.flush();
        let mut calls = if self.complete {
            self.calls()
        } else {
            Vec::new()
        };
        let modules = self.modules.iter().map(|module| {
            let end = module.first.saturating_add(module.count);
            let procedures = self.procedures.get(module.first..end).unwrap_or_default();
            let positions = module.first..end;
            let links = procedures
                .iter()
                .zip(positions)
                .map(|(info, index)| ProcedureLinks {
                    is_entry_point: self.is_entry_point(info),
                    possibly_unused: self.complete.then(|| self.is_unused(info)),
                    calls: calls.get_mut(index).and_then(Option::take),
                });
            links.collect()
        });
        modules.collect()
    }

    /// Registers a procedure of a module in its group, which it starts when it
    /// is the first of its name, and gives the group. `groups` holds the groups
    /// of the module so far, by name.
    fn declare(
        &mut self,
        module_type: ModuleType,
        procedure: &Procedure,
        groups: &mut BTreeMap<usize, usize>,
    ) -> usize {
        let name = self.name_id(&procedure.name);
        let existing = name.and_then(|name| groups.get(&name).copied());
        let group = existing.unwrap_or(self.groups.len());
        if existing.is_none() {
            self.groups.push(Group::new(name));
            if let Some(name) = name {
                groups.insert(name, group);
                self.names.declare(name, &procedure.name);
            }
        }
        if module_type == ModuleType::Standard && procedure.is_public() {
            self.make_visible(group);
        }
        group
    }

    /// The number of a name, if the graph can keep it: `None` for what VBA takes
    /// for no name, and for a name that is past the limit on names, which
    /// leaves the graph incomplete.
    fn name_id(&mut self, name: &str) -> Option<usize> {
        match self.names.intern(name, self.limits.name_bytes) {
            Interned::Name(id) => Some(id),
            Interned::NotAName => None,
            Interned::Full => {
                self.complete = false;
                None
            }
        }
    }

    /// Makes the group one that other modules can call by its name.
    fn make_visible(&mut self, group: usize) {
        let Some(group) = self.groups.get_mut(group) else {
            return;
        };
        if !group.visible {
            group.visible = true;
            if let Some(name) = group.name {
                self.names.count_visible(name);
            }
        }
    }

    /// The group of `module` that has the name, if there is one.
    fn local_group(&self, module: usize, name: usize) -> Option<usize> {
        self.modules.get(module)?.groups.get(&name).copied()
    }

    /// Moves to the statements of another procedure: what the one before
    /// referred to is kept.
    fn switch_to(&mut self, procedure: usize) {
        if self.current != Some(procedure) {
            self.flush();
            self.current = Some(procedure);
        }
    }

    /// Keeps the references of the procedure that was being read.
    fn flush(&mut self) {
        self.edges.extend(std::mem::take(&mut self.pending));
    }

    /// Takes note that the caller mentions a name.
    fn mention(&mut self, caller: Caller, name: &str, how: Mention) {
        if self.mentions == 0 {
            self.complete = false;
            return;
        }
        self.mentions = self.mentions.saturating_sub(1);
        let Some(name) = self.name_id(name) else {
            return;
        };
        let referrers = match how {
            Mention::Bare => match self.local_group(caller.module, name) {
                Some(group) => self
                    .groups
                    .get_mut(group)
                    .map(|group| &mut group.from_inside),
                None => self.names.referrers(name, how),
            },
            Mention::Named => self.names.referrers(name, how),
        };
        if let Some(referrers) = referrers {
            referrers.add(caller.group);
        }
        self.refer(Edge {
            caller: caller.index,
            name,
            how,
        });
    }

    /// Keeps a reference, once.
    fn refer(&mut self, edge: Edge) {
        if self.pending.contains(&edge) {
            return;
        }
        if self.edges.len().saturating_add(self.pending.len()) >= self.limits.references {
            self.complete = false;
            return;
        }
        self.pending.insert(edge);
    }

    /// The names that each procedure calls, by procedure, once the references
    /// are all known. A procedure whose list does not fit has none.
    fn calls(&mut self) -> Vec<Option<Vec<String>>> {
        self.edges.sort_unstable();
        self.edges.dedup();
        let mut calls = vec![Some(Vec::new()); self.procedures.len()];
        let mut total = 0_usize;
        for edges in self
            .edges
            .chunk_by(|left, right| left.caller == right.caller)
        {
            let Some(caller) = edges.first().map(|edge| edge.caller) else {
                continue;
            };
            let mut called: Vec<usize> = (edges.iter())
                .filter(|edge| self.reaches_another_group(edge))
                .map(|edge| edge.name)
                .collect();
            called.dedup();
            let mut spelled: Vec<&str> = (called.iter())
                .filter_map(|&name| self.names.spelling(name))
                .collect();
            spelled.sort_unstable();
            let fits = spelled.len() <= self.limits.calls_per_procedure
                && total.saturating_add(spelled.len()) <= self.limits.calls;
            if fits {
                total = total.saturating_add(spelled.len());
            }
            if let Some(slot) = calls.get_mut(caller) {
                *slot = fits.then(|| spelled.into_iter().map(str::to_owned).collect());
            }
        }
        calls
    }

    /// Whether the reference names a group other than the caller's own, which
    /// is what makes it a call.
    fn reaches_another_group(&self, edge: &Edge) -> bool {
        let Some(caller) = self.procedures.get(edge.caller) else {
            return false;
        };
        let Some(info) = self.names.info(edge.name) else {
            return false;
        };
        match edge.how {
            Mention::Bare => match self.local_group(caller.module, edge.name) {
                Some(group) => group != caller.group,
                None => info.visible > 0,
            },
            Mention::Named => {
                let own = self.groups.get(caller.group).and_then(|group| group.name);
                info.declared > usize::from(own == Some(edge.name))
            }
        }
    }

    /// Whether the host may start the procedure, or a user: a procedure is one
    /// with the other procedures of its group.
    fn is_entry_point(&self, procedure: &ProcedureInfo) -> bool {
        (self.groups.get(procedure.group)).is_some_and(|group| group.entry)
    }

    /// The structural predicate `possibly_unused`, for a procedure of a graph
    /// that was read in full: it is not an entry point, and no group but its own
    /// refers to its name in a way that reaches it.
    fn is_unused(&self, procedure: &ProcedureInfo) -> bool {
        if self.is_entry_point(procedure) {
            return false;
        }
        let Some(group) = self.groups.get(procedure.group) else {
            return false;
        };
        let Some(info) = group.name.and_then(|name| self.names.info(name)) else {
            // A procedure with no name can be referred to by none.
            return true;
        };
        let own = procedure.group;
        let referred = group.from_inside.has_any_but(own)
            || (group.visible && info.from_outside.has_any_but(own))
            || info.named.has_any_but(own);
        !referred
    }
}

/// How a statement refers to a procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mention {
    /// By a bare name, as `Helper x` does: the procedure is in the module of the
    /// statement or is one that every module can call.
    Bare,
    /// By a name that tells no more: after a dot, as a member, in a string. It
    /// may be any procedure of that name.
    Named,
}

/// A reference of a procedure to a name, and how it refers to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    /// The position of the procedure in the graph, from its first module on.
    caller: usize,
    /// The name in the form in which names compare.
    name: usize,
    /// How the procedure refers to it.
    how: Mention,
}

/// The procedure whose statement is read.
#[derive(Debug, Clone, Copy)]
struct Caller {
    /// Its position in the graph.
    index: usize,
    /// Its module.
    module: usize,
    /// Its group.
    group: usize,
}

/// A procedure of the graph.
#[derive(Debug, Clone, Copy)]
struct ProcedureInfo {
    module: usize,
    group: usize,
    /// The position of its header among the logical lines of its module.
    header: usize,
}

/// A module of the graph.
#[derive(Debug)]
struct ModuleInfo {
    /// The position of its first procedure in the graph.
    first: usize,
    /// How many procedures it has.
    count: usize,
    /// Its groups, by the name of the group.
    groups: BTreeMap<usize, usize>,
}

impl ModuleInfo {
    /// The position in the graph of the procedure that comes `position`th in
    /// the module, if it has one.
    fn index(&self, position: usize) -> Option<usize> {
        (position < self.count).then(|| self.first.saturating_add(position))
    }
}

/// The procedures of one module that share a name.
#[derive(Debug)]
struct Group {
    /// Its name, if VBA could take it for one.
    name: Option<usize>,
    /// Other modules can call it by its name: it holds a public procedure of a
    /// standard module.
    visible: bool,
    /// The host may start it, or a user: it holds a procedure that is an entry
    /// point.
    entry: bool,
    /// The groups that refer to it by a bare name from its own module.
    from_inside: Referrers,
}

impl Group {
    fn new(name: Option<usize>) -> Self {
        Self {
            name,
            visible: false,
            entry: false,
            from_inside: Referrers::default(),
        }
    }
}

/// Who refers to something, as far as one question needs: whether a group
/// other than a given one does.
#[derive(Debug, Clone, Copy, Default)]
struct Referrers {
    /// The first group that did.
    first: Option<usize>,
    /// A second one did.
    more: bool,
}

impl Referrers {
    /// Takes note that `group` refers to it.
    fn add(&mut self, group: usize) {
        match self.first {
            None => self.first = Some(group),
            Some(first) if first != group => self.more = true,
            Some(_) => {}
        }
    }

    /// Whether a group other than `group` refers to it.
    fn has_any_but(self, group: usize) -> bool {
        self.more || self.first.is_some_and(|first| first != group)
    }
}

/// The names of the graph, in the form in which names compare: in lower case.
#[derive(Debug, Default)]
struct Names {
    ids: BTreeMap<String, usize>,
    infos: Vec<NameInfo>,
    /// The bytes that the keys of `ids` take together.
    bytes: usize,
    /// The key being looked up.
    key: String,
}

/// What the graph knows of a name.
#[derive(Debug, Default)]
struct NameInfo {
    /// The name as the project spells it: the smallest spelling, in byte order,
    /// among the procedures that declare it.
    spelling: Option<String>,
    /// How many groups declare it.
    declared: usize,
    /// How many of them other modules can call by that name.
    visible: usize,
    /// The groups that refer to it by a bare name where their module declares
    /// none of it.
    from_outside: Referrers,
    /// The groups that refer to it by a name alone.
    named: Referrers,
}

/// What interning a name came to.
#[derive(Debug, Clone, Copy)]
enum Interned {
    /// The name has this number.
    Name(usize),
    /// VBA takes it for no name: it is empty or too long.
    NotAName,
    /// There is no room for another name.
    Full,
}

impl Names {
    /// The number of `name`, which it gets if it has none and if the keys keep
    /// within `max_bytes`.
    fn intern(&mut self, name: &str, max_bytes: usize) -> Interned {
        if name.is_empty() || name.chars().nth(MAX_NAME_CHARS).is_some() {
            return Interned::NotAName;
        }
        self.key.clear();
        self.key.extend(name.chars().flat_map(char::to_lowercase));
        if let Some(&id) = self.ids.get(self.key.as_str()) {
            return Interned::Name(id);
        }
        let bytes = self.bytes.saturating_add(self.key.len());
        if bytes > max_bytes {
            return Interned::Full;
        }
        let id = self.infos.len();
        self.ids.insert(self.key.clone(), id);
        self.infos.push(NameInfo::default());
        self.bytes = bytes;
        Interned::Name(id)
    }

    /// Takes note that a group declares `name`, which `spelling` spells. The
    /// name keeps the smallest spelling, in byte order, that a group gives it,
    /// so that the order of the modules changes nothing.
    fn declare(&mut self, name: usize, spelling: &str) {
        if let Some(info) = self.infos.get_mut(name) {
            info.declared = info.declared.saturating_add(1);
            if info.spelling.as_deref().is_none_or(|kept| spelling < kept) {
                info.spelling = Some(spelling.to_owned());
            }
        }
    }

    /// Takes note that other modules can call a group of `name` by it.
    fn count_visible(&mut self, name: usize) {
        if let Some(info) = self.infos.get_mut(name) {
            info.visible = info.visible.saturating_add(1);
        }
    }

    /// What the graph knows of `name`.
    fn info(&self, name: usize) -> Option<&NameInfo> {
        self.infos.get(name)
    }

    /// Who refer to `name` the way `how` says, when no group of the module of
    /// the statement declares a name for a bare reference to reach.
    fn referrers(&mut self, name: usize, how: Mention) -> Option<&mut Referrers> {
        let info = self.infos.get_mut(name)?;
        Some(match how {
            Mention::Bare => &mut info.from_outside,
            Mention::Named => &mut info.named,
        })
    }

    /// The name as the project declares it, if it does.
    fn spelling(&self, name: usize) -> Option<&str> {
        self.info(name)?.spelling.as_deref()
    }
}

/// Whether the host may start the procedure by itself or a user may start it
/// from the outside (see the module documentation).
fn is_entry_point(module_type: ModuleType, procedure: &Procedure) -> bool {
    match module_type {
        ModuleType::Standard => {
            procedure.is_public()
                && (procedure.kind == ProcedureKind::Function || procedure.parameter_count == 0)
        }
        ModuleType::Class => {
            (CLASS_EVENTS.iter()).any(|event| procedure.name.eq_ignore_ascii_case(event))
        }
        ModuleType::Document | ModuleType::Form => false,
    }
}

/// The members of the statement that are names and that nothing invokes: a
/// variable, a property, a procedure that is named and not called. The facts
/// list every call as a member too, in the same order, so what is left of the
/// members once the calls are paired with them, in order, is what nothing
/// invokes. The name of an argument is a member that is no call (`Pattern:=1`)
/// and is left out before it can be paired with the call of a name that is
/// the same (`Color:=Color`).
fn uninvoked(facts: &Facts) -> impl Iterator<Item = &MemberUse> {
    let mut calls = facts.calls.iter().peekable();
    facts.members.iter().filter(move |member| {
        !member.named_argument && calls.next_if(|call| call.name == member.name).is_none()
    })
}

/// Whether the statement starts a procedure from its name, given as a string
/// (see [`DISPATCHERS`]).
fn starts_by_name(facts: &Facts) -> bool {
    let is_dispatcher = |name: &str| DISPATCHERS.iter().any(|d| name.eq_ignore_ascii_case(d));
    facts
        .members
        .iter()
        .any(|member| is_dispatcher(&member.name))
}

/// The name of the procedure that a string names: the identifier after its last
/// `.` or `!`, which drops the workbook and the module that may qualify it, as
/// `"'Book1.xlsm'!Module1.Helper"` does.
fn macro_name(text: &str) -> Option<&str> {
    if text.len() > MAX_STRING_BYTES {
        return None;
    }
    let name = text.rsplit(['.', '!']).next()?;
    let mut chars = name.chars();
    let starts_a_name = chars.next().is_some_and(char::is_alphabetic);
    (starts_a_name && chars.all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Visibility;
    use crate::vba::{CallSite, MemberUse};

    /// A `Sub` with no parameter, whose header is the line 0 and which has one
    /// statement, the line 1.
    fn procedure(name: &str, visibility: Visibility) -> Procedure {
        Procedure {
            name: name.to_owned(),
            kind: ProcedureKind::Sub,
            visibility,
            is_static: false,
            parameter_count: 0,
            start_line: 1,
            end_line: 3,
            lines: 0..3,
            recoveries: Vec::new(),
        }
    }

    fn private(name: &str) -> Procedure {
        procedure(name, Visibility::Private)
    }

    /// The facts of a statement that calls the names, bare.
    fn calling(names: &[&str]) -> Facts {
        Facts {
            members: (names.iter())
                .map(|&name| MemberUse {
                    name: name.to_owned(),
                    named_argument: false,
                })
                .collect(),
            calls: (names.iter())
                .map(|&name| CallSite {
                    name: name.to_owned(),
                    after_dot: false,
                })
                .collect(),
            ..Facts::default()
        }
    }

    /// What a graph that keeps no more than `limits` says of the procedures of
    /// a standard module, once the statements have been read: each one is the
    /// position of a procedure and the names that its statement calls.
    fn run(
        limits: Limits,
        procedures: &[Procedure],
        statements: &[(usize, &[&str])],
    ) -> Vec<ProcedureLinks> {
        let mut graph = CallGraph::with_limits(limits);
        let module = graph.add_module(ModuleType::Standard, procedures, &[]);
        for &(procedure, names) in statements {
            graph.read(module, procedure, 1, &calling(names));
        }
        graph.resolve().into_iter().flatten().collect()
    }

    fn unused(links: &[ProcedureLinks]) -> Vec<Option<bool>> {
        links.iter().map(|links| links.possibly_unused).collect()
    }

    #[test]
    fn names_past_the_limit_leave_the_links_unknown_but_not_the_entry_points() {
        let limits = Limits {
            name_bytes: 8,
            ..LIMITS
        };
        // Eight bytes of names are kept, nine are not.
        let within = run(limits, &[private("aaaa"), private("bbbb")], &[]);
        assert_eq!(unused(&within), [Some(true), Some(true)]);

        let past = [
            private("aaaa"),
            private("bbbb"),
            procedure("c", Visibility::Public),
        ];
        let links = run(limits, &past, &[]);
        assert_eq!(unused(&links), [None, None, None]);
        assert!(links.iter().all(|links| links.calls.is_none()));
        let entries: Vec<bool> = links.iter().map(|links| links.is_entry_point).collect();
        assert_eq!(entries, [false, false, true]);

        // A name that a statement mentions counts as well.
        let links = run(limits, &[private("aaaa")], &[(0, &["aaaa", "bbbb", "c"])]);
        assert_eq!(unused(&links), [None]);
    }

    #[test]
    fn mentions_past_the_limit_leave_the_links_unknown() {
        let limits = Limits {
            mentions: 3,
            ..LIMITS
        };
        let procedures = [private("a"), private("b")];
        // `a` refers to itself, which is no use; `b` is used.
        let within = run(limits, &procedures, &[(0, &["a", "b", "x"])]);
        assert_eq!(unused(&within), [Some(true), Some(false)]);
        assert_eq!(within[0].calls, Some(vec!["b".to_owned()]));

        let past = run(limits, &procedures, &[(0, &["a", "b", "x", "y"])]);
        assert_eq!(unused(&past), [None, None]);
        assert!(past.iter().all(|links| links.calls.is_none()));
    }

    #[test]
    fn references_past_the_limit_leave_the_links_unknown() {
        let limits = Limits {
            references: 2,
            ..LIMITS
        };
        let procedures = [private("a"), private("b")];
        // A name that the procedure mentions again is the same reference.
        let within = run(limits, &procedures, &[(0, &["b", "b", "x", "b"])]);
        assert_eq!(unused(&within), [Some(true), Some(false)]);

        let past = run(limits, &procedures, &[(0, &["b", "x", "y"])]);
        assert_eq!(unused(&past), [None, None]);
    }

    #[test]
    fn a_list_of_calls_that_is_too_long_is_unknown_and_the_rest_is_not() {
        let limits = Limits {
            calls_per_procedure: 2,
            ..LIMITS
        };
        let names = ["caller", "a", "b", "c", "other"];
        let procedures: Vec<_> = names.iter().map(|&name| private(name)).collect();
        let statements: [(usize, &[&str]); 2] = [(0, &["a", "b", "c"]), (4, &["a", "b"])];
        let links = run(limits, &procedures, &statements);
        assert_eq!(links[0].calls, None);
        assert_eq!(links[1].calls, Some(Vec::new()));
        assert_eq!(links[4].calls, Some(vec!["a".to_owned(), "b".to_owned()]));
        // What refers to what is still known.
        assert_eq!(
            unused(&links),
            [
                Some(true),
                Some(false),
                Some(false),
                Some(false),
                Some(true)
            ]
        );
    }

    #[test]
    fn the_lists_of_calls_together_keep_within_the_limit() {
        let limits = Limits { calls: 3, ..LIMITS };
        let names = ["p", "q", "r", "a", "b", "c"];
        let procedures: Vec<_> = names.iter().map(|&name| private(name)).collect();
        // The first list takes two of the three. The second does not fit; the
        // third, which is one name, does.
        let statements: [(usize, &[&str]); 3] = [(0, &["a", "b"]), (1, &["a", "b"]), (2, &["c"])];
        let links = run(limits, &procedures, &statements);
        assert_eq!(links[0].calls, Some(vec!["a".to_owned(), "b".to_owned()]));
        assert_eq!(links[1].calls, None);
        assert_eq!(links[2].calls, Some(vec!["c".to_owned()]));
        assert_eq!(links[3].calls, Some(Vec::new()));
    }

    #[test]
    fn the_members_that_nothing_invokes_are_left_once_the_calls_are_paired_in_order() {
        let member = |name: &str, named_argument: bool| MemberUse {
            name: name.to_owned(),
            named_argument,
        };
        let call = |name: &str| CallSite {
            name: name.to_owned(),
            after_dot: false,
        };
        let left = |facts: &Facts| -> Vec<String> {
            uninvoked(facts).map(|member| member.name.clone()).collect()
        };

        // `Workbooks.Open Filename:=Filename`: the name of the argument is no
        // call, and it does not take the place of the value, which is.
        let facts = Facts {
            members: vec![
                member("Workbooks", false),
                member("Open", false),
                member("Filename", true),
                member("Filename", false),
            ],
            calls: vec![call("Filename")],
            ..Facts::default()
        };
        assert_eq!(left(&facts), ["Workbooks", "Open"]);

        // A call that no member answers (the facts do not do that) leaves every
        // member after it: the graph errs on the side of a name being used.
        let facts = Facts {
            members: vec![member("a", false), member("b", false)],
            calls: vec![call("c"), call("b")],
            ..Facts::default()
        };
        assert_eq!(left(&facts), ["a", "b"]);
    }

    #[test]
    fn an_incomplete_graph_reads_no_more() {
        let limits = Limits {
            mentions: 1,
            ..LIMITS
        };
        let mut graph = CallGraph::with_limits(limits);
        let module = graph.add_module(ModuleType::Standard, &[private("a")], &[]);
        graph.read(module, 0, 1, &calling(&["a", "b"]));
        assert!(!graph.complete);
        let pending = graph.pending.len();
        graph.read(module, 0, 1, &calling(&["c"]));
        assert_eq!(graph.pending.len(), pending);
    }
}
