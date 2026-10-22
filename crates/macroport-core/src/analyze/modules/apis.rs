// SPDX-License-Identifier: Apache-2.0
//! The APIs that each procedure of a file uses (SPEC M2 item 3, ADR-0011): what
//! its statements show of the object model, of the functions it calls and of the
//! objects it creates, each name with how many times the procedure uses it.
//!
//! A list holds three kinds of name:
//!
//! - **A member chain**, as the one dotted name that it makes: `Workbooks.Open`,
//!   `Range.Interior.Color`. The arguments of the calls in a chain are left out,
//!   and a chain that continues the object of a `With` block starts with that
//!   object. Its first name can be a variable (`ws.Cells.Clear`): the front end
//!   reads no declaration, so the name is listed as the code spells it.
//! - **A ProgID**, with the function that creates the object:
//!   `CreateObject:Scripting.Dictionary`, `GetObject:Word.Application`.
//! - **A function called by name**, with no dot before it (`Shell`, `Trim`), if
//!   a `call` matcher of the rules in use names it or the module declares it
//!   with `Declare`. Any other name that stands alone is a variable, a
//!   constant, a procedure of the project or a function that no rule cares for,
//!   and a statement does not say which, so none is listed. The lists
//!   therefore depend on the rules in use. A function that the VBA library
//!   qualifies (`VBA.Strings.Replace`, ADR-0010) has a dot before it: it is
//!   listed as its chain, which says what it is, and not again by its name.
//!   The same holds for a call with arguments that a dot follows
//!   (`Intersect(a, b).Address`): it starts a chain, which is listed, and the
//!   call is not.
//!
//! VBA ignores the case of names, so names that differ by it are one entry,
//! spelled as its first use spells it. A list is in the order of its names with
//! their case folded, whatever the order of the statements. The statements
//! outside the procedures belong to no list.
//!
//! A hostile file can make as many distinct names as it has text, so the lists
//! are bounded: one holds at most [`MAX_APIS`] names, and the lists of a file
//! take [`MAX_API_MIB`] MiB together. A list that would pass a bound is left out
//! of the report (`None`) and not cut, since a short list would read as a whole
//! one; the module that holds it gets a warning of the file for that
//! (`vba.apis.left_out`).

use std::collections::{BTreeMap, BTreeSet};

use super::Analysis;
use crate::report::ApiUse;
use crate::rules::{Matcher, RuleSet};
use crate::vba::{CallSite, DeclarationKind, Facts, PROGID_FUNCTIONS};

/// The most distinct names that the list of one procedure holds. Real
/// procedures use a few dozen.
pub(super) const MAX_APIS: usize = 1_000;

/// What the lists of one file may take together, in the weight that
/// [`List::add`] gives an entry, in MiB.
pub(super) const MAX_API_MIB: usize = 4;

/// [`MAX_API_MIB`] in the unit of the weights.
const MAX_API_BYTES: usize = MAX_API_MIB * 1024 * 1024;

/// What an entry weighs besides its name and the folded copy of it that keys
/// it: the nodes of the map, the two strings and the count.
const ENTRY_WEIGHT: usize = 64;

/// What stands before a ProgID when the facts of its statement do not say
/// which function created the object.
const UNNAMED_FUNCTION: &str = "ProgID";

/// What the lists of the procedures of a file share: the functions that the
/// rules name, and the room that is left for the entries.
#[derive(Debug)]
pub(super) struct FileApis {
    /// The functions that a list names when a statement calls one with no dot
    /// before it, with their case folded.
    functions: BTreeSet<String>,
    /// What the lists of the modules read so far have left to take.
    room: usize,
}

impl FileApis {
    /// The lists of a file read with `rules`: the functions are the names that
    /// a `call` matcher of a family or of a security signal lists.
    pub(super) fn new(rules: &RuleSet) -> Self {
        let in_families = (rules.families.families.iter()).flat_map(|family| &family.signals);
        let matchers = (in_families.map(|signal| &signal.matcher))
            .chain(rules.security.signals.iter().map(|signal| &signal.matcher));
        let functions = matchers
            .filter_map(|matcher| match matcher {
                Matcher::Call(names) => Some(names),
                _ => None,
            })
            .flatten()
            .map(|name| folded(name))
            .collect();
        Self {
            functions,
            room: MAX_API_BYTES,
        }
    }

    /// The lists of the procedures of the module `analysis` holds, which the
    /// statements of the module fill in. The module also knows the functions
    /// that it declares.
    pub(super) fn module(&mut self, analysis: &Analysis) -> ModuleApis<'_> {
        let declared = (analysis.module.declarations.iter())
            .filter_map(|declaration| match &declaration.kind {
                DeclarationKind::Declare { name, .. } if !name.is_empty() => Some(folded(name)),
                _ => None,
            })
            .collect();
        let procedures = analysis.module.procedures.len();
        ModuleApis {
            file: self,
            declared,
            lists: std::iter::repeat_with(List::default)
                .take(procedures)
                .collect(),
            key: String::new(),
        }
    }
}

/// The lists of the procedures of one module, while its statements are read.
#[derive(Debug)]
pub(super) struct ModuleApis<'f> {
    file: &'f mut FileApis,
    /// The functions that the module declares with `Declare`, with their case
    /// folded. A routine that another module declares is not known here: the
    /// front end reads one module at a time.
    declared: BTreeSet<String>,
    /// One list for each procedure of the module, in order.
    lists: Vec<List>,
    /// The folded name of the call being looked up. A statement holds many
    /// calls and few are functions that the lists name, so the name of each is
    /// folded into this one buffer and not into a string of its own.
    key: String,
}

impl ModuleApis<'_> {
    /// Adds the APIs that `facts`, the facts of a statement of the procedure at
    /// position `procedure` among those of the module, show.
    pub(super) fn add(&mut self, procedure: usize, facts: &Facts) {
        let Self {
            file,
            declared,
            lists,
            key,
        } = self;
        let Some(list) = lists.get_mut(procedure).filter(|list| !list.left_out) else {
            return;
        };
        for chain in &facts.chains {
            list.add(&mut file.room, chain.join("."));
        }
        // How many of the chains start with each name, made when a call needs it.
        let mut starts = None;
        for call in facts.calls.iter().filter(|call| !call.after_dot) {
            fold_into(key, &call.name);
            if !file.functions.contains(key.as_str()) && !declared.contains(key.as_str()) {
                continue;
            }
            // A call with arguments that a dot follows starts a chain, which is
            // listed for it: the call is left out once for each chain of the
            // statement that starts with its name.
            let heads = starts.get_or_insert_with(|| chain_heads(&facts.chains));
            if let Some(left) = heads.get_mut(key.as_str()).filter(|left| **left > 0) {
                *left = left.saturating_sub(1);
                continue;
            }
            list.add(&mut file.room, call.name.clone());
        }
        if !facts.progids.is_empty() {
            let creators = creators(facts);
            for (creator, progid) in creators.iter().zip(&facts.progids) {
                list.add(&mut file.room, format!("{creator}:{progid}"));
            }
        }
    }

    /// The list of each procedure of the module, in order: `None` for one that
    /// passed a bound.
    pub(super) fn finish(self) -> Vec<Option<Vec<ApiUse>>> {
        self.lists.into_iter().map(List::finish).collect()
    }
}

/// The list of one procedure.
#[derive(Debug, Default)]
struct List {
    /// The entries, by the name with its case folded.
    uses: BTreeMap<String, ApiUse>,
    /// What the entries took from the room of the file.
    taken: usize,
    /// The list passed a bound: it holds nothing, and takes nothing.
    left_out: bool,
}

impl List {
    /// Counts one use of the API `name`. A name that is new takes room from
    /// `room`, that of the file. If the list has [`MAX_APIS`] entries, or the
    /// room cannot hold the new one, the list is left out: it gives back what
    /// it took, so that the lists after it can use it.
    fn add(&mut self, room: &mut usize, name: String) {
        if self.left_out {
            return;
        }
        let key = folded(&name);
        if let Some(known) = self.uses.get_mut(&key) {
            known.count = known.count.saturating_add(1);
            return;
        }
        let weight = ENTRY_WEIGHT
            .saturating_add(name.len())
            .saturating_add(key.len());
        if self.uses.len() >= MAX_APIS || weight > *room {
            *room = room.saturating_add(self.taken);
            self.taken = 0;
            self.uses = BTreeMap::new();
            self.left_out = true;
            return;
        }
        *room = room.saturating_sub(weight);
        self.taken = self.taken.saturating_add(weight);
        self.uses.insert(key, ApiUse { name, count: 1 });
    }

    /// The entries in order, or `None` if the list was left out.
    fn finish(self) -> Option<Vec<ApiUse>> {
        (!self.left_out).then(|| self.uses.into_values().collect())
    }
}

/// `name` with its case folded: VBA ignores it.
fn folded(name: &str) -> String {
    let mut folded = String::with_capacity(name.len());
    fold_into(&mut folded, name);
    folded
}

/// Writes `name` with its case folded into `into`, in place of what it held.
fn fold_into(into: &mut String, name: &str) {
    into.clear();
    into.extend(name.chars().flat_map(char::to_lowercase));
}

/// How many of `chains` start with each name, the name with its case folded.
fn chain_heads(chains: &[Vec<String>]) -> BTreeMap<String, usize> {
    let mut heads = BTreeMap::new();
    for head in chains.iter().filter_map(|chain| chain.first()) {
        let count = heads.entry(folded(head)).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    heads
}

/// Whether `call` is to a function of the library of VBA, or can be: it has no
/// dot before it, or a name of the library qualifies it. The method of another
/// object (`app.CreateObject`) is none.
fn is_library_call(call: &CallSite) -> bool {
    !call.after_dot || call.library
}

/// The function that created each ProgID of `facts`, in the order of the
/// ProgIDs. The facts list the ProgIDs and the names of the calls apart, and a
/// call whose class does not fold gives no ProgID, so the two meet only when
/// they are as many: each ProgID then has the function at its own position.
/// Otherwise the function is known if all the calls that create an object are
/// to the same one, and else the ProgIDs have no function ([`UNNAMED_FUNCTION`]).
/// The calls that count are those of the library: the statement reader gives no
/// ProgID to the method of an object, and a method would put the others out of
/// step.
fn creators(facts: &Facts) -> Vec<&'static str> {
    let named: Vec<&'static str> = (facts.calls.iter())
        .filter(|call| is_library_call(call))
        .filter_map(|call| {
            (PROGID_FUNCTIONS.iter())
                .map(|&(function, _)| function)
                .find(|function| function.eq_ignore_ascii_case(&call.name))
        })
        .collect();
    if named.len() == facts.progids.len() {
        return named;
    }
    let one = named
        .first()
        .filter(|first| named.iter().all(|other| other == *first));
    vec![one.copied().unwrap_or(UNNAMED_FUNCTION); facts.progids.len()]
}

#[cfg(test)]
mod tests {
    use super::super::statements::each_statement;
    use super::*;
    use crate::rules::RulesSources;

    fn rules() -> RuleSet {
        RuleSet::load(&RulesSources::EMBEDDED).expect("the shipped rules load")
    }

    /// The facts of the first statement of the module `text`.
    fn facts(text: &str) -> Facts {
        let analysis = Analysis::of(text).expect("a short module");
        let mut found = Vec::new();
        each_statement(&analysis, |_, _, _, facts| found.push(facts));
        found.into_iter().next().expect("a statement")
    }

    /// The entries of a list as `(name, count)` pairs, `None` if it was left out.
    fn pairs(list: Option<Vec<ApiUse>>) -> Option<Vec<(String, u32)>> {
        list.map(|uses| uses.into_iter().map(|u| (u.name, u.count)).collect())
    }

    /// `(name, count)` pairs from a literal.
    fn entries(of: &[(&str, u32)]) -> Option<Vec<(String, u32)>> {
        Some(
            of.iter()
                .map(|&(name, count)| (name.to_owned(), count))
                .collect(),
        )
    }

    /// A list that took `names` one after the other from `room`.
    fn list_of(room: &mut usize, names: &[&str]) -> List {
        let mut list = List::default();
        for name in names {
            list.add(room, (*name).to_owned());
        }
        list
    }

    /// What an entry weighs when its name is one letter, in the room of a file.
    const ONE_LETTER: usize = ENTRY_WEIGHT + 2;

    #[test]
    fn the_functions_are_the_names_of_the_call_matchers_of_the_rules_in_lower_case() {
        let file = FileApis::new(&rules());

        // `Trim` and `Len` are named by a family, `CallByName` and
        // `ExecuteExcel4Macro` by a security signal and by no family.
        for name in ["trim", "len", "callbyname", "executeexcel4macro"] {
            assert!(file.functions.contains(name), "{name}");
        }
        assert!(!file.functions.contains("Trim"));
        assert_eq!(file.room, MAX_API_BYTES);
    }

    #[test]
    fn the_chains_of_a_statement_are_counted_by_the_name_they_start_with() {
        let chains = [
            ["Intersect", "Address"],
            ["intersect", "Count"],
            ["Range", "Value"],
        ]
        .map(|chain| chain.map(str::to_owned).to_vec());

        let heads = chain_heads(&chains);

        let expected = [("intersect", 2), ("range", 1)];
        let found: Vec<_> = heads
            .iter()
            .map(|(name, count)| (name.as_str(), *count))
            .collect();
        assert_eq!(found, expected);
        assert!(chain_heads(&[]).is_empty());
    }

    #[test]
    fn a_list_with_more_names_than_the_most_is_left_out_and_not_cut() {
        let mut room = MAX_API_BYTES;
        let mut list = List::default();
        for number in 0..MAX_APIS {
            list.add(&mut room, format!("a.b{number}"));
        }

        // A name that it holds is counted again, and that is no new entry.
        list.add(&mut room, "A.B0".to_owned());
        assert_eq!(list.uses.len(), MAX_APIS);
        assert_eq!(list.uses.get("a.b0").map(|known| known.count), Some(2));
        // One more name leaves it out whole.
        list.add(&mut room, "a.b1000".to_owned());

        assert_eq!(pairs(list.finish()), None);
    }

    #[test]
    fn a_list_that_the_room_of_the_file_cannot_hold_is_left_out_and_gives_back_what_it_took() {
        // Room for two entries of one letter and not for a third.
        let mut room = 3 * ONE_LETTER - 1;

        let list = list_of(&mut room, &["a", "b", "c"]);

        assert_eq!(pairs(list.finish()), None);
        assert_eq!(room, 3 * ONE_LETTER - 1);
        // The next list takes the room that it gave back.
        let next = list_of(&mut room, &["x", "y"]);
        assert_eq!(pairs(next.finish()), entries(&[("x", 1), ("y", 1)]));
        assert_eq!(room, ONE_LETTER - 1);
    }

    #[test]
    fn a_list_that_is_left_out_takes_nothing_more() {
        let mut room = 100;
        let mut list = list_of(&mut room, &[&"a".repeat(100)]);

        // It fits the room, but the list is left out and stays so.
        list.add(&mut room, "b".to_owned());

        assert_eq!(room, 100);
        assert_eq!(pairs(list.finish()), None);
    }

    #[test]
    fn the_facts_of_a_procedure_that_the_module_does_not_have_are_ignored() {
        let analysis = Analysis::of("Sub A()\nEnd Sub\n").expect("a short module");
        let mut file = FileApis::new(&rules());
        let mut module = file.module(&analysis);

        module.add(1, &facts("x = a.b"));
        module.add(0, &facts("x = a.b"));

        let lists: Vec<_> = module.finish().into_iter().map(pairs).collect();
        assert_eq!(lists, [entries(&[("a.b", 1)])]);
    }

    /// The functions that made the ProgIDs of the statement `text`.
    fn creators_of(text: &str) -> Vec<&'static str> {
        creators(&facts(text))
    }

    #[test]
    fn a_progid_is_made_by_the_function_that_the_statement_calls() {
        assert_eq!(
            creators_of("Set o = CreateObject(\"A.B\")"),
            ["CreateObject"]
        );
        assert_eq!(creators_of("Set o = GetObject(, \"A.B\")"), ["GetObject"]);
        // Whatever the case of the call, the name is that of the library.
        assert_eq!(creators_of("Set o = getobject(, \"A.B\")"), ["GetObject"]);
        // The library may qualify it: the call has a dot before it all the same.
        let qualified = "Set o = VBA.Interaction.CreateObject(\"A.B\")";
        assert_eq!(creators_of(qualified), ["CreateObject"]);
        assert_eq!(creators_of("Set o = Shell(\"A.B\")"), Vec::<&str>::new());
    }

    #[test]
    fn the_method_of_an_object_is_no_creator_and_does_not_put_the_others_out_of_step() {
        let method = "Set o = app.CreateObject(\"A.B\")";
        // The class of the method gives no ProgID, and the other call gives one.
        let mixed = "x = IIf(c, app.CreateObject(\"A.B\"), GetObject(, \"C.D\"))";

        assert_eq!(creators_of(method), Vec::<&str>::new());
        assert_eq!(creators_of(mixed), ["GetObject"]);
    }

    #[test]
    fn each_progid_of_a_statement_has_the_function_at_its_own_position() {
        let both = "x = IIf(c, CreateObject(\"A.B\"), GetObject(, \"C.D\"))";
        let reversed = "x = IIf(c, GetObject(, \"A.B\"), CreateObject(\"C.D\"))";

        assert_eq!(creators_of(both), ["CreateObject", "GetObject"]);
        assert_eq!(creators_of(reversed), ["GetObject", "CreateObject"]);
    }

    #[test]
    fn when_a_class_does_not_fold_the_function_is_known_only_if_every_call_is_to_one() {
        // Two calls and one ProgID: the class of the first is a variable.
        let same = "x = IIf(c, CreateObject(n), CreateObject(\"A.B\"))";
        let other = "x = IIf(c, CreateObject(n), GetObject(, \"A.B\"))";

        assert_eq!(creators_of(same), ["CreateObject"]);
        assert_eq!(creators_of(other), [UNNAMED_FUNCTION]);
    }
}
