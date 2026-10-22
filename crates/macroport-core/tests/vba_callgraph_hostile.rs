// SPDX-License-Identifier: Apache-2.0
//! Hostile projects for the call graph. Building one in time linear in its size
//! shows on the clock only, not in the result: the limit is set far above what a
//! linear reading takes and far below what a quadratic one would take. Splitting
//! the lines, parsing and reading the facts are not part of the time.

// clippy.toml keeps the clock out of the core, not out of its tests, and
// excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::expect_used
)]

mod vba_callgraph_common;

use std::time::{Duration, Instant};

use macroport_core::rules::ModuleType;
use macroport_core::vba::{CallGraph, MAX_TOKENS, ProcedureLinks};
use vba_callgraph_common::Module;

/// What building the graph may take: about ten times what a debug build takes
/// for the largest case here.
const LIMIT: Duration = Duration::from_secs(10);

/// The links of the procedures of each module, once the graph has read every
/// module as the front end does.
fn links(modules: &[(ModuleType, String)]) -> Vec<Vec<ProcedureLinks>> {
    let prepared: Vec<Module> = (modules.iter())
        .map(|(module_type, text)| Module::read(*module_type, text))
        .collect();
    let started = Instant::now();
    let mut graph = CallGraph::new();
    for module in &prepared {
        module.add_to(&mut graph, &[]);
    }
    let links = graph.resolve();
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    links
}

/// The links of a project of one standard module.
fn standard(text: String) -> Vec<ProcedureLinks> {
    links(&[(ModuleType::Standard, text)])
        .into_iter()
        .next()
        .expect("a module")
}

#[test]
fn many_procedures_of_one_name_are_one_group() {
    let links = standard("Private Sub Same()\n    Same\nEnd Sub\n".repeat(20_000));
    assert_eq!(links.len(), 20_000);
    assert!(
        links
            .iter()
            .all(|links| links.possibly_unused == Some(true))
    );
    assert!(links.iter().all(|links| links.calls == Some(Vec::new())));
}

#[test]
fn many_modules_that_share_a_name_are_read_in_linear_time() {
    // Each of the modules refers to `Helper` and every one of the others
    // declares it: a pairwise reading would look at a hundred million pairs.
    let module = "Private Sub Helper()\nEnd Sub\nSub Main()\n    o.Helper\nEnd Sub\n";
    let modules: Vec<_> = (0..10_000)
        .map(|_| (ModuleType::Standard, module.to_owned()))
        .collect();
    let links = links(&modules);
    assert_eq!(links.len(), 10_000);
    for module in &links {
        let [helper, main] = module.as_slice() else {
            panic!("two procedures");
        };
        assert_eq!(helper.possibly_unused, Some(false));
        assert_eq!(main.calls, Some(vec!["Helper".to_owned()]));
    }
}

#[test]
fn a_statement_of_many_names_is_read_in_linear_time() {
    // A hundred thousand names take 590 KB, within the limit on names.
    let names: Vec<String> = (0..100_000).map(|at| format!("n{at}")).collect();
    let statement = format!("x = {}", names.join(" + "));
    let text = format!("Sub Main()\n{statement}\nEnd Sub\nPrivate Sub n99999()\nEnd Sub\n");
    let links = standard(text);
    let [main, found] = links.as_slice() else {
        panic!("two procedures");
    };
    assert_eq!(main.calls, Some(vec!["n99999".to_owned()]));
    assert_eq!(found.possibly_unused, Some(false));
}

#[test]
fn names_past_the_limit_on_names_leave_the_links_unknown() {
    // Two hundred thousand names take 1.3 MB, over the megabyte of names that
    // the graph keeps. It leaves the links unknown and tells the entry points.
    let names: Vec<String> = (0..200_000).map(|at| format!("n{at}")).collect();
    let statement = format!("x = {}", names.join(" + "));
    let text = format!("Sub Main()\n{statement}\nEnd Sub\nPrivate Sub n199999()\nEnd Sub\n");
    let links = standard(text);
    let [main, found] = links.as_slice() else {
        panic!("two procedures");
    };
    assert!(main.is_entry_point);
    assert_eq!(main.calls, None);
    assert_eq!(found.possibly_unused, None);
}

#[test]
fn a_name_repeated_in_a_statement_is_one_reference() {
    let statement = format!("x = {}0", "o.Helper + ".repeat(200_000));
    let text = format!("Sub Main()\n{statement}\nEnd Sub\nPrivate Sub Helper()\nEnd Sub\n");
    let links = standard(text);
    assert_eq!(links[0].calls, Some(vec!["Helper".to_owned()]));
    assert_eq!(links[1].possibly_unused, Some(false));
}

#[test]
fn what_follows_the_token_limit_of_a_statement_is_not_read() {
    // The lexer keeps the first `MAX_TOKENS` tokens of a line and makes one of
    // the rest.
    let filler = "1 + ".repeat(MAX_TOKENS);
    let statement = format!("x = Before + {filler}0 + After");
    let text = format!("Sub Main()\n{statement}\nEnd Sub\n");
    let text = format!("{text}Private Sub Before()\nEnd Sub\nPrivate Sub After()\nEnd Sub\n");
    let links = standard(text);
    assert_eq!(links[0].calls, Some(vec!["Before".to_owned()]));
    assert_eq!(links[1].possibly_unused, Some(false));
    assert_eq!(links[2].possibly_unused, Some(true));
}

#[test]
fn a_long_chain_of_calls_is_resolved_in_linear_time() {
    // 20,000 procedures that call the next five: a hundred thousand references.
    let count = 20_000_usize;
    let mut text = String::from("Sub Main()\n    P0\nEnd Sub\n");
    for at in 0..count {
        text.push_str(&format!("Private Sub P{at}()\n"));
        for next in 1..=5_usize {
            text.push_str(&format!("    P{}\n", at.saturating_add(next)));
        }
        text.push_str("End Sub\n");
    }
    let links = standard(text);
    assert_eq!(links.len(), count.saturating_add(1));
    assert_eq!(links[0].calls, Some(vec!["P0".to_owned()]));
    assert!(
        links
            .iter()
            .all(|links| links.possibly_unused == Some(false))
    );
    // The last procedures call names that the project does not declare.
    let last = links.last().expect("a procedure");
    assert_eq!(last.calls, Some(Vec::new()));
    let first = links.get(1).expect("a procedure");
    assert_eq!(first.calls.as_ref().map(Vec::len), Some(5));
}

#[test]
fn procedures_read_by_turns_are_resolved_in_linear_time() {
    let text = "Sub Main()\n    First\nEnd Sub\nSub Other()\n    Second\nEnd Sub\n\
                Private Sub First()\nEnd Sub\nPrivate Sub Second()\nEnd Sub\n";
    let parsed = Module::read(ModuleType::Standard, text);
    // The call of each of the first two procedures.
    let main = &parsed.statements[0][1];
    let other = &parsed.statements[1][1];

    let started = Instant::now();
    let mut graph = CallGraph::new();
    let module = graph.add_module(ModuleType::Standard, &parsed.procedures, &[]);
    for _ in 0..20_000 {
        graph.read(module, 0, main.0, &main.1);
        graph.read(module, 1, other.0, &other.1);
    }
    let links = graph.resolve();
    let elapsed = started.elapsed();
    assert!(elapsed < LIMIT, "took {elapsed:?}");
    assert_eq!(links[0][0].calls, Some(vec!["First".to_owned()]));
    assert_eq!(links[0][1].calls, Some(vec!["Second".to_owned()]));
}

#[test]
fn names_that_vba_would_not_accept_are_not_kept() {
    let long = "x".repeat(300);
    let statement: Vec<String> = (0..5000).map(|at| format!("{long}{at}")).collect();
    let text = format!(
        "Sub Main()\n    y = {}\nEnd Sub\nPrivate Sub {long}()\nEnd Sub\n",
        statement.join(" + ")
    );
    let links = standard(text);
    // The procedure with a name of 300 characters is no procedure to call.
    assert_eq!(links[0].calls, Some(Vec::new()));
    assert_eq!(links[1].possibly_unused, Some(true));
}

#[test]
fn a_string_of_megabytes_given_to_a_call_is_not_read_as_a_name() {
    let name = "a".repeat(2_000_000);
    let text = format!("Sub Main()\n    Application.Run \"{name}\"\nEnd Sub\n");
    let links = standard(text);
    assert_eq!(links[0].calls, Some(Vec::new()));
}
