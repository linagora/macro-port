// SPDX-License-Identifier: Apache-2.0
//! The call graph on the samples of the corpus (SPEC M2 item 5). Each sample is
//! a project of one module, then they are one project together, then Excel's own
//! exports of the samples stand for them. What the cases expect comes from
//! reading the samples, not from running the graph on them.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

mod vba_callgraph_common;

use std::collections::{BTreeMap, BTreeSet};

use macroport_core::rules::ModuleType;
use macroport_core::vba::{CallGraph, ProcedureLinks};
use macroport_testkit::corpus;
use serde::Deserialize;
use vba_callgraph_common::Module;

/// For each procedure of a module, whether the host starts it by itself. The
/// engine will tell the graph (SPEC section 7); this stands in for it with the
/// names of the events of the objects of a workbook and of a form, which are all
/// that the samples need, and the names of the auto-exec procedures. It does not
/// know the controls of a form or the `WithEvents` variables.
fn host_starts(module: &Module) -> Vec<bool> {
    const AUTO_EXEC: [&str; 5] = [
        "auto_open",
        "auto_close",
        "autoopen",
        "autoclose",
        "autoexec",
    ];
    const OBJECTS: [&str; 3] = ["workbook_", "worksheet_", "chart_"];
    let started = |name: &str| {
        let name = name.to_lowercase();
        let event = match module.module_type {
            ModuleType::Document => OBJECTS.iter().any(|object| name.starts_with(object)),
            ModuleType::Form => name.starts_with("userform_"),
            ModuleType::Standard | ModuleType::Class => false,
        };
        event || AUTO_EXEC.contains(&name.as_str())
    };
    (module.procedures.iter())
        .map(|procedure| started(&procedure.name))
        .collect()
}

/// What the graph says of each procedure of each module of a project. When
/// `started` is false the host starts nothing.
fn project(modules: &[&Module], started: bool) -> Vec<Vec<ProcedureLinks>> {
    let mut graph = CallGraph::new();
    for module in modules {
        let flags = if started {
            host_starts(module)
        } else {
            Vec::new()
        };
        module.add_to(&mut graph, &flags);
    }
    graph.resolve()
}

/// The links of the procedures of a module, each with the name of its procedure.
fn named(module: &Module, links: &[ProcedureLinks]) -> Vec<(String, ProcedureLinks)> {
    assert_eq!(links.len(), module.procedures.len());
    let names = (module.procedures.iter()).map(|procedure| procedure.name.clone());
    names.zip(links.iter().cloned()).collect()
}

/// A module taken as the whole project: for each procedure, its name and what
/// the graph says of it.
fn alone(module: &Module, started: bool) -> Vec<(String, ProcedureLinks)> {
    let links = project(&[module], started).into_iter().next();
    named(module, &links.expect("the links of the module"))
}

/// The links of the procedure called `name`, which the module declares once.
fn find<'a>(links: &'a [(String, ProcedureLinks)], name: &str) -> &'a ProcedureLinks {
    let found: Vec<_> = links.iter().filter(|(found, _)| found == name).collect();
    assert_eq!(found.len(), 1, "{name} is declared {} times", found.len());
    let found = found.first().copied().expect("the procedure");
    &found.1
}

/// The names of the procedures of a module that are possibly unused.
fn unused(links: &[(String, ProcedureLinks)]) -> Vec<&str> {
    let unused = links
        .iter()
        .filter(|(_, links)| links.possibly_unused == Some(true));
    unused.map(|(name, _)| name.as_str()).collect()
}

/// The calls of a procedure of a graph that was read in full.
fn calls(links: &ProcedureLinks) -> Vec<&str> {
    let calls = links.calls.as_ref();
    let calls = calls.expect("the calls of a graph read in full");
    calls.iter().map(String::as_str).collect()
}

/// A text sample of the corpus.
struct Sample {
    /// Its path, relative to `corpus/`.
    path: String,
    /// Its number, such as `09-04`.
    number: String,
    module: Module,
}

/// The keys of `fixtures.yaml` that these tests use.
#[derive(Deserialize)]
struct Fixtures {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    /// The sample that Excel made the workbook from, if there is one.
    source: Option<String>,
    #[serde(default)]
    modules: Vec<FixtureModule>,
}

#[derive(Deserialize)]
struct FixtureModule {
    #[serde(rename = "type")]
    module_type: ModuleType,
    expected_source: Option<String>,
}

/// What the fixtures say of a text sample.
struct Exported {
    /// The type of the module that the sample becomes in a workbook.
    module_type: ModuleType,
    /// The exports that Excel made of that module, in each kind of workbook:
    /// their paths relative to `corpus/`.
    exports: Vec<String>,
}

/// What `fixtures.yaml` says of each text sample that has fixtures, by its path
/// relative to `corpus/`.
fn exported() -> BTreeMap<String, Exported> {
    let text = String::from_utf8(corpus::read("fixtures/fixtures.yaml")).expect("UTF-8");
    // The file starts with a byte order mark.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let fixtures = serde_saphyr::from_str::<Fixtures>(text).expect("the fixtures file reads");
    let mut found: BTreeMap<String, Exported> = BTreeMap::new();
    for fixture in fixtures.fixtures {
        let Some(source) = fixture.source else {
            continue;
        };
        let modules = fixture.modules.into_iter();
        let mut modules = modules.filter(|module| module.expected_source.is_some());
        let module = modules.next().expect("an exported module");
        assert!(modules.next().is_none(), "one exported module for {source}");
        let path = format!("fixtures/{}", module.expected_source.unwrap_or_default());
        let entry = found.entry(source).or_insert_with(|| Exported {
            module_type: module.module_type,
            exports: Vec::new(),
        });
        assert_eq!(entry.module_type, module.module_type);
        entry.exports.push(path);
    }
    found
}

/// The text of a corpus file. The exports are in code page 1252; only comments
/// and strings are out of ASCII there.
fn text(path: &str) -> String {
    String::from_utf8_lossy(&corpus::read(path)).into_owned()
}

/// Every text sample of the corpus.
fn samples() -> Vec<Sample> {
    let exported = exported();
    let paths = corpus::files("families", &["bas", "cls", "frm"]);
    assert!(!paths.is_empty(), "no sample in the corpus");
    let samples = paths.into_iter().map(|path| {
        // Excel cannot make a form from code, so the forms have no fixture.
        let module_type = if path.ends_with(".frm") {
            ModuleType::Form
        } else {
            exported.get(&path).expect("a fixture").module_type
        };
        let file = path.rsplit('/').next().unwrap_or_default();
        Sample {
            number: file.chars().take("00-00".len()).collect(),
            module: Module::read(module_type, &text(&path)),
            path,
        }
    });
    samples.collect()
}

/// The module of the sample numbered `number`.
fn sample<'a>(samples: &'a [Sample], number: &str) -> &'a Module {
    let mut found = samples.iter().filter(|sample| sample.number == number);
    let sample = found.next().expect("the sample");
    assert!(found.next().is_none(), "two samples for {number}");
    &sample.module
}

#[test]
fn public_procedures_that_a_button_or_a_cell_may_run_are_entry_points() {
    let samples = samples();
    let links = |number| alone(sample(&samples, number), true);

    // `Sub editTable()` takes no parameter.
    assert!(find(&links("01-06"), "editTable").is_entry_point);
    // A library of functions for the cells of a sheet.
    let functions = links("08-01");
    assert!(!functions.is_empty());
    assert!(functions.iter().all(|(_, links)| links.is_entry_point));
    // `Example4` takes no parameter and `readFile` is a function. `writeFile`
    // is a `Sub` that takes parameters, and `Example4` calls it.
    let csv = links("10-05");
    assert!(find(&csv, "Example4").is_entry_point);
    assert!(find(&csv, "readFile").is_entry_point);
    assert!(!find(&csv, "writeFile").is_entry_point);
    assert_eq!(find(&csv, "writeFile").possibly_unused, Some(false));
    // `Sub Auto_Open()` is an auto-exec procedure and takes no parameter.
    assert!(find(&links("14-01"), "Auto_Open").is_entry_point);
}

#[test]
fn a_procedure_that_nothing_refers_to_is_possibly_unused() {
    let samples = samples();
    let links = |number| alone(sample(&samples, number), true);

    // The one call of `SaveAsToFolderPath` is commented out, and the others
    // are called: `editTable` calls the first, and the second calls the third.
    let table = links("01-06");
    assert_eq!(unused(&table), ["SaveAsToFolderPath"]);
    assert_eq!(
        calls(find(&table, "editTable")),
        [
            "AutoSumColumnAF",
            "changeProperties",
            "removeHeaderSpace",
            "removeProductCodeSpace"
        ]
    );
    assert_eq!(
        calls(find(&table, "AutoSumColumnAF")),
        ["SelectFirstEmptyRowInColumn"]
    );

    // The functions that set their own result refer to nothing but themselves.
    assert_eq!(unused(&links("08-04")), ["SumOnlyVisible"]);
    assert_eq!(unused(&links("10-01")), ["LoopThroughFiles"]);
    let export = links("06-01");
    assert_eq!(
        unused(&export),
        [
            "EXPORT_PDF_MultipleSheet_ForOne",
            "Export_Excel_MultipleSheet_ForOne",
            "CreateSaveFileName"
        ]
    );

    // `writeFile2` is public but takes parameters, and no one calls it.
    let csv = links("10-05");
    assert_eq!(unused(&csv), ["writeFile2"]);
    assert!(!find(&csv, "writeFile2").is_entry_point);

    // Of the forty-two private procedures of a library, one is not called.
    assert_eq!(unused(&links("10-06")), ["CreateODDiagnosticsFile"]);
    // All the private procedures of a JSON converter are.
    assert_eq!(unused(&links("11-05")), [] as [&str; 0]);
}

#[test]
fn a_procedure_that_a_string_names_is_used_by_the_procedure_that_holds_the_string() {
    // `StartBlinking` schedules itself with `Application.OnTime`, which is no
    // use of itself, and `StopBlinking` cancels the schedule by the same name.
    // Nothing calls `StopBlinking`.
    let samples = samples();
    let links = alone(sample(&samples, "09-04"), true);
    assert_eq!(unused(&links), ["StopBlinking"]);
    assert_eq!(calls(find(&links, "StartBlinking")), [] as [&str; 0]);
    assert_eq!(calls(find(&links, "StopBlinking")), ["StartBlinking"]);
}

#[test]
fn calls_name_the_procedures_that_a_procedure_calls_by_the_name_the_project_gives() {
    let samples = samples();
    let links = |number| alone(sample(&samples, number), true);

    // `VersionRename` calls `File_Renamer` twice, which makes one call in the
    // list, and `File_Renamer` calls nothing of the sample.
    let rename = links("10-04");
    assert_eq!(calls(find(&rename, "VersionRename")), ["File_Renamer"]);
    assert_eq!(calls(find(&rename, "File_Renamer")), [] as [&str; 0]);
    assert!(unused(&rename).is_empty());

    let mail = links("12-02");
    assert_eq!(
        calls(find(&mail, "GenerateInfoToCreateEmail")),
        ["SendEmail"]
    );
    assert_eq!(calls(find(&mail, "SendEmail")), ["GetBoiler"]);

    // `test_EXPORTPDFOneByOne` takes no parameter and calls `EXPORTPDFOneByOne`,
    // which calls `GetNextAvailableName`.
    let export = links("06-01");
    assert_eq!(
        calls(find(&export, "test_EXPORTPDFOneByOne")),
        ["EXPORTPDFOneByOne"]
    );
    assert_eq!(
        calls(find(&export, "EXPORTPDFOneByOne")),
        ["GetNextAvailableName"]
    );
}

#[test]
fn event_handlers_are_entry_points_only_because_the_host_starts_them() {
    // Each of these document modules holds one private procedure, the handler
    // of an event of the workbook or of a sheet: `Workbook_SheetChange`,
    // `Worksheet_Change`, `Worksheet_SelectionChange`, `Workbook_BeforeSave`
    // and `Workbook_Open`, which is also an auto-exec procedure.
    let samples = samples();
    for number in ["09-02", "09-03", "09-05", "09-06", "14-02"] {
        let module = sample(&samples, number);
        assert_eq!(module.module_type, ModuleType::Document, "{number}");
        assert_eq!(module.procedures.len(), 1, "{number}");

        let started = alone(module, true);
        assert!(started.iter().all(|(_, links)| links.is_entry_point));
        assert!(unused(&started).is_empty(), "{number}");

        // Nothing calls them, so if the host did not start them they would be
        // unused.
        let left = alone(module, false);
        assert!(left.iter().all(|(_, links)| !links.is_entry_point));
        assert_eq!(unused(&left).len(), 1, "{number}");
    }

    // The same goes for the handlers of a form, which nothing calls either. The
    // handler of each control is one more that the engine knows.
    let form = sample(&samples, "05-01");
    assert_eq!(form.module_type, ModuleType::Form);
    let started = alone(form, true);
    for name in ["UserForm_Initialize", "UserForm_Terminate"] {
        assert!(find(&started, name).is_entry_point, "{name}");
    }
    let left = alone(form, false);
    for name in ["UserForm_Initialize", "UserForm_Terminate"] {
        assert!(!find(&left, name).is_entry_point, "{name}");
        assert_eq!(find(&left, name).possibly_unused, Some(true), "{name}");
    }
}

#[test]
fn what_the_graph_says_of_every_sample_holds_together() {
    for sample in samples() {
        let module = &sample.module;
        let declared: BTreeSet<&str> = (module.procedures.iter())
            .map(|procedure| procedure.name.as_str())
            .collect();
        for (name, links) in alone(module, true) {
            let path = &sample.path;
            // An entry point is not unused, and the graph read the whole sample.
            if links.is_entry_point {
                assert_eq!(links.possibly_unused, Some(false), "{path} {name}");
            }
            assert!(links.possibly_unused.is_some(), "{path} {name}");
            // The calls are sorted and once, name procedures of the sample in
            // the spelling it uses, and leave out the procedure itself.
            let called = calls(&links);
            let mut sorted = called.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(called, sorted, "{path} {name}");
            for call in called {
                assert!(declared.contains(call), "{path} {name} calls {call}");
                assert!(!call.eq_ignore_ascii_case(&name), "{path} {name}");
            }
        }
    }
}

#[test]
fn the_facts_of_every_statement_list_the_calls_among_the_members_in_order() {
    // The graph tells the names that nothing invokes by pairing the calls with
    // the members, in order. It relies on the facts listing each call as a
    // member, in the same order, and on no name of an argument being a call.
    let mut statements = 0_usize;
    for sample in samples() {
        for (line, facts) in sample.module.statements.iter().flatten() {
            let mut members = facts.members.iter().filter(|member| !member.named_argument);
            for call in &facts.calls {
                let found = members.any(|member| member.name == call.name);
                assert!(found, "{} line {line}: {call:?}", sample.path);
            }
            statements = statements.saturating_add(1);
        }
    }
    assert!(statements > 0);
}

#[test]
fn a_form_that_calls_the_export_library_uses_its_procedures() {
    // The form of sample 05-05 is the face of the export library of sample
    // 06-01: its buttons call the procedures that nothing in the library calls.
    let samples = samples();
    let library = sample(&samples, "06-01");
    let form = sample(&samples, "05-05");
    let library_alone = alone(library, true);
    let unused_alone = [
        "EXPORT_PDF_MultipleSheet_ForOne",
        "Export_Excel_MultipleSheet_ForOne",
        "CreateSaveFileName",
    ];
    assert_eq!(unused(&library_alone), unused_alone);

    for flipped in [false, true] {
        let modules = if flipped {
            [form, library]
        } else {
            [library, form]
        };
        let mut links = project(&modules, true);
        assert_eq!(links.len(), 2);
        if flipped {
            links.reverse();
        }
        let form_links = named(form, &links.pop().expect("the form"));
        let library_links = named(library, &links.pop().expect("the library"));

        // The buttons call the two procedures that were unused, and the private
        // one stays unused: only its module can call it.
        assert_eq!(unused(&library_links), ["CreateSaveFileName"]);
        assert_eq!(
            calls(find(&form_links, "cbOkExport_Click")),
            [
                "EXPORTExcelOneByOne",
                "EXPORTPDFOneByOne",
                "EXPORT_PDF_MultipleSheet_ForOne",
                "Export_Excel_MultipleSheet_ForOne",
                "GetSelectedSheet"
            ]
        );
        assert_eq!(
            calls(find(&form_links, "UserForm_Initialize")),
            ["getExportName", "getListSheet"]
        );
    }
}

#[test]
fn the_samples_together_are_one_project_in_any_order() {
    let samples = samples();
    let modules: Vec<&Module> = samples.iter().map(|sample| &sample.module).collect();
    let together = project(&modules, true);
    assert_eq!(together.len(), modules.len());

    // The order of the modules changes nothing.
    let reversed: Vec<&Module> = modules.iter().rev().copied().collect();
    let mut again = project(&reversed, true);
    again.reverse();
    for ((sample, left), right) in samples.iter().zip(&together).zip(&again) {
        assert_eq!(left, right, "{}", sample.path);
    }

    // More modules never make a procedure more unused, and never take a call
    // away: they may name a procedure that a module did not have.
    for ((sample, module), together) in samples.iter().zip(&modules).zip(&together) {
        let alone = project(&[*module], true).into_iter().next();
        for (before, after) in alone.expect("a module").iter().zip(together) {
            let path = &sample.path;
            assert_eq!(before.is_entry_point, after.is_entry_point, "{path}");
            assert!(after.possibly_unused <= before.possibly_unused, "{path}");
            let kept: BTreeSet<String> =
                (calls(after).into_iter()).map(str::to_lowercase).collect();
            for call in calls(before) {
                assert!(kept.contains(&call.to_lowercase()), "{path} lost {call}");
            }
        }
    }
}

#[test]
fn the_exports_that_excel_made_of_the_samples_give_the_same_links() {
    let samples = samples();
    let exported = exported();
    let mut compared = 0_usize;
    for sample in &samples {
        // Excel cannot make a form from code, so the forms have no export.
        let Some(exported) = exported.get(&sample.path) else {
            assert!(
                sample.path.ends_with(".frm"),
                "{} has no export",
                sample.path
            );
            continue;
        };
        // One workbook of each of the three kinds.
        assert_eq!(exported.exports.len(), 3, "{}", sample.path);
        let expected = alone(&sample.module, true);
        for path in &exported.exports {
            let module = Module::read(exported.module_type, &text(path));
            assert_eq!(alone(&module, true), expected, "{path}");
            compared = compared.saturating_add(1);
        }
    }
    assert_eq!(compared, exported.len().saturating_mul(3));
}
