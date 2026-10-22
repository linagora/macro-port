// SPDX-License-Identifier: Apache-2.0
//! A snapshot of what the front end makes of each text sample of the corpus
//! (SPEC M2, "Done when": insta snapshots over `corpus/families/**`), one file
//! for each sample in `snapshots/`.
//!
//! A snapshot has two parts. The first is what `analyze()` reports for the
//! sample: the status, the error codes and the traits of the file, the metrics
//! of the file, of its module and of each procedure, the declarations of the
//! module, and the fields of each procedure that a later stage fills in. The
//! second, under `parse:`, is what the parser reads in the text: its
//! declarations, its procedures with whether their blocks match, and the places
//! where it went on past a statement that it could not read. A procedure gets
//! the trait `compile_error_suspected` for one of those two causes, so the
//! second part is where to look for why, unless a statement nests too deep to be
//! read, which the error `vba.statement.too_deep` of the first part says. The
//! spans of the procedures are in both parts on purpose: they come from two
//! layers, and a snapshot where they differ shows it.
//!
//! In a line, `-` stands for a field that the report leaves out because no stage
//! fills it in yet or because it passed a bound, and `none` for an empty list.
//! A metric that the report leaves out is not printed. `calls` prints as a comma
//! separated list once something fills it: that format is a guess, and the
//! change that wires it in may adjust it. `apis` prints how many names a
//! procedure uses, then a line `api name*count` for each below the procedure,
//! in the order of the report, so that a change of one name is a change of one
//! line. The families and the signals are the rule engine's and not the front
//! end's, so they are not in the snapshots.
//!
//! The renderers destructure the report and the parser's types without `..`, so
//! a field added to one of them does not compile until the rendering decides
//! what to do with it.
//!
//! A change of the parser or of the metrics moves the snapshots that it touches.
//! `INSTA_UPDATE=always cargo test -p macroport-core --test analyze
//! each_sample_matches_its_snapshot` writes them. Read the diff of `snapshots/`
//! and take only the moves that you can explain: a line that changes is a
//! decision of the parser, never something to accept blindly. In CI nothing is
//! written, and a snapshot that is missing or different fails the test.

use std::path::Path;

use insta::{Settings, assert_snapshot, glob};
use macroport_core::report::{ApiUse, Declarations, FileReport, Metrics, ProcedureReport};
use macroport_core::vba::{self, Declaration, ParsedModule, Procedure, Recovery, SourceLines};
use serde::Serialize;

use crate::support::{codes, corpus_report, modules, sample_text, samples};

// `glob!` lists the corpus with `Path::canonicalize`, which `clippy.toml` bans
// in the core (CLAUDE.md: a core test that reads files allows it).
#[allow(clippy::disallowed_methods)]
#[test]
fn each_sample_matches_its_snapshot() {
    // Every snapshot would repeat the same `expression:` line.
    let mut settings = Settings::clone_current();
    settings.set_omit_expression(true);
    settings.bind(|| {
        glob!(
            "../../../../corpus/families",
            "**/*.{bas,cls,frm}",
            |path| {
                let relative = corpus_relative(path);
                let file = corpus_report(&relative);

                let mut lines = render_report(&file);
                lines.extend(render_parse(&sample_text(&relative)));
                assert_snapshot!(lines.join("\n"));
            }
        );
    });
    // After the loop, so that it reads the directory once the snapshots are
    // written.
    assert_snapshots_are_those_of_the_samples();
}

/// Fails unless every sample has a snapshot and every snapshot is of a sample.
/// `glob!` only visits the samples it finds, so the snapshot of a sample that
/// was renamed or removed would stay behind unnoticed.
#[allow(clippy::disallowed_methods)]
fn assert_snapshots_are_those_of_the_samples() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analyze/snapshots");
    let mut of_snapshots = Vec::new();
    for entry in std::fs::read_dir(dir).expect("the directory of the snapshots") {
        let path = entry.expect("an entry of the directory").path();
        if path.extension().is_none_or(|extension| extension != "snap") {
            continue;
        }
        let snapshot = std::fs::read_to_string(&path).expect("a snapshot is UTF-8");
        // The header names the sample, relative to the workspace.
        let header = (snapshot.lines()).find_map(|line| line.strip_prefix("input_file: "));
        let sample = header.and_then(|input| input.strip_prefix("corpus/"));
        of_snapshots.push(sample.expect("the sample of a snapshot").to_owned());
    }
    let of_samples = samples();

    let without_snapshot: Vec<_> = (of_samples.iter())
        .filter(|sample| !of_snapshots.contains(sample))
        .collect();
    let without_sample: Vec<_> = (of_snapshots.iter())
        .filter(|sample| !of_samples.contains(sample))
        .collect();
    assert_eq!(without_snapshot, Vec::<&String>::new(), "no snapshot");
    assert_eq!(without_sample, Vec::<&String>::new(), "no sample");
    assert_eq!(of_snapshots.len(), of_samples.len(), "one for each sample");
}

/// The path of a file of `corpus/` relative to it, with `/` separators.
fn corpus_relative(path: &Path) -> String {
    let parts: Vec<_> = (path.components())
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let corpus = parts.iter().rposition(|part| part == "corpus");
    let below = corpus.and_then(|at| parts.get(at + 1..));
    below.expect("a path below corpus/").join("/")
}

/// The name that the report gives to `value`: `no_macros`, `text_module`.
fn wire_name<T: Serialize>(value: &T) -> String {
    let value = serde_json::to_value(value).expect("a serialisable value");
    value.as_str().expect("a name").to_owned()
}

/// `value`, or `-` when the report leaves the field out.
fn or_dash<T: ToString>(value: Option<T>) -> String {
    value.map_or("-".to_owned(), |value| value.to_string())
}

/// `items` joined by commas, or `none` when there are none.
fn join_or_none<I>(items: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let items: Vec<_> = items.into_iter().collect();
    if items.is_empty() {
        return "none".to_owned();
    }
    let items: Vec<_> = items.iter().map(AsRef::as_ref).collect();
    items.join(",")
}

/// `items` joined by commas, or `none` when there are none, or `-` when the
/// report leaves the list out.
fn join_or_dash<T: AsRef<str>>(items: Option<&[T]>) -> String {
    items.map_or("-".to_owned(), join_or_none)
}

/// The metrics that the report holds, as `name=value` pairs: the two that every
/// metrics object has, then the others that a stage has filled in.
fn show_metrics(metrics: &Metrics) -> String {
    let Metrics {
        loc,
        logical_loc,
        procedures,
        cyclomatic,
        cyclomatic_total,
        max_nesting,
        parameters,
        goto_count,
        forms,
        declare_count,
        external_dependencies,
    } = metrics;
    let optional = [
        ("procedures", procedures),
        ("cyclomatic", cyclomatic),
        ("cyclomatic_total", cyclomatic_total),
        ("max_nesting", max_nesting),
        ("parameters", parameters),
        ("goto_count", goto_count),
        ("forms", forms),
        ("declare_count", declare_count),
        ("external_dependencies", external_dependencies),
    ];
    let mut pairs = vec![format!("loc={loc}"), format!("logical_loc={logical_loc}")];
    for (name, value) in optional {
        if let Some(value) = value {
            pairs.push(format!("{name}={value}"));
        }
    }
    pairs.join(" ")
}

/// A procedure of the report as a line: its kind, name, visibility and span, its
/// metrics and traits, and the fields that the stages after the parser fill in.
/// A line for each of its APIs follows.
fn show_procedure(procedure: &ProcedureReport) -> Vec<String> {
    let ProcedureReport {
        name,
        kind,
        visibility,
        is_entry_point,
        is_event_handler,
        possibly_unused,
        start_line,
        end_line,
        metrics,
        calls,
        apis,
        traits,
        // The rule engine's: left out.
        primary_family: _,
        secondary_families: _,
        families: _,
        signals: _,
        orientation: _,
    } = procedure;
    let line = format!(
        "  {} {name} {} lines {start_line}-{end_line}: {} traits={} entry_point={} \
         event_handler={} possibly_unused={} calls={} apis={}",
        wire_name(kind),
        visibility.as_ref().map_or("-".to_owned(), wire_name),
        show_metrics(metrics),
        join_or_none(traits.iter().flatten()),
        or_dash(*is_entry_point),
        or_dash(*is_event_handler),
        or_dash(*possibly_unused),
        join_or_dash(calls.as_deref()),
        or_dash(apis.as_ref().map(Vec::len)),
    );
    let uses = apis.iter().flatten();
    let uses = uses.map(|ApiUse { name, count }| format!("    api {name}*{count}"));
    std::iter::once(line).chain(uses).collect()
}

/// The lines of the snapshot that come from the report: the file, then each
/// module with its procedures.
fn render_report(file: &FileReport) -> Vec<String> {
    let file_metrics = (file.metrics.as_ref()).map_or("-".to_owned(), show_metrics);
    let mut lines = vec![
        file.path.clone(),
        format!("status: {}", wire_name(&file.status)),
        format!("container: {}", wire_name(&file.container)),
        format!("errors: {}", join_or_none(codes(file))),
        format!("traits: {}", join_or_none(file.traits.iter().flatten())),
        format!("file: {file_metrics}"),
    ];
    for module in modules(file) {
        let header = format!("module {} ({})", module.name, module.module_type.name());
        lines.push(format!("{header}: {}", show_metrics(&module.metrics)));
        lines.push(match &module.declarations {
            Some(declarations) => show_declarations(declarations),
            None => "  declarations: -".to_owned(),
        });
        lines.extend(module.procedures.iter().flat_map(show_procedure));
    }
    lines
}

/// The declarations section of a module of the report.
fn show_declarations(declarations: &Declarations) -> String {
    let Declarations {
        declare_count,
        with_events_count,
        option_explicit,
        option_vba_support,
    } = declarations;
    format!(
        "  declarations: declare_count={} with_events_count={} option_explicit={} \
         option_vba_support={}",
        or_dash(*declare_count),
        or_dash(*with_events_count),
        or_dash(*option_explicit),
        or_dash(*option_vba_support),
    )
}

/// The places where the parser went on, as `Kind@line` with the debug form of
/// the kind (so that a kind it adds shows), or `none`.
fn show_recoveries(recoveries: &[Recovery]) -> String {
    let places = recoveries
        .iter()
        .map(|Recovery { kind, line }| format!("{kind:?}@{line}"));
    join_or_none(places)
}

/// A procedure as the parser reads it, with whether its blocks match, which is
/// measured as `analyze()` does.
fn show_parsed_procedure(source: &SourceLines, procedure: &Procedure) -> String {
    let Procedure {
        name,
        kind,
        visibility,
        is_static,
        parameter_count,
        start_line,
        end_line,
        lines,
        recoveries,
    } = procedure;
    let code = source.logical.get(lines.clone());
    let code = code.expect("the logical lines of a procedure");
    let measure = vba::measure_procedure(*start_line, *end_line, *parameter_count, code);
    format!(
        "  procedure {} {name} {} static={is_static} parameters={parameter_count} \
         lines {start_line}-{end_line} recoveries={} unbalanced_blocks={}",
        wire_name(kind),
        wire_name(visibility),
        show_recoveries(recoveries),
        measure.unbalanced_blocks,
    )
}

/// The lines of the snapshot that come from the parser: what it reads in
/// `text`, with the debug form of the kinds of its declarations.
fn render_parse(text: &str) -> Vec<String> {
    let source = vba::split_lines(text).expect("a sample has few enough lines");
    let ParsedModule {
        procedures,
        declarations,
        recoveries,
    } = vba::parse(&source);
    let mut out = vec!["parse:".to_owned()];
    for Declaration {
        kind,
        line,
        lines: _,
    } in &declarations
    {
        out.push(format!("  declaration {kind:?} line {line}"));
    }
    for procedure in &procedures {
        out.push(show_parsed_procedure(&source, procedure));
    }
    out.push(format!("  recoveries: {}", show_recoveries(&recoveries)));
    out
}
