// SPDX-License-Identifier: Apache-2.0
//! The front end on the corpus (ACCEPTANCE A2), through `analyze()`: the text
//! samples, and the workbooks and the exports of their modules that Excel made
//! of them.

use macroport_core::analyze::analyze;
use macroport_testkit::corpus;

use crate::support::{checked, modules, options, suspected};

#[test]
fn a_procedure_whose_blocks_do_not_match_gets_the_trait_in_its_sample_and_in_its_fixtures() {
    // See `vba_metrics_corpus.rs`: what makes their blocks not match.
    for (family, sample, extension, name) in [
        (
            "09-evenements",
            "09-03-horodatage-statut",
            "cls",
            "Worksheet_Change",
        ),
        (
            "13-api-windows-activex",
            "13-05-getobject-findwindow-sendmessage",
            "bas",
            "GetExcel",
        ),
    ] {
        let mut paths = vec![format!("families/{family}/{sample}.{extension}")];
        for format in ["xls", "xlsb", "xlsm"] {
            paths.push(format!("fixtures/generated/{sample}.{format}"));
            paths.push(format!(
                "fixtures/generated/{sample}.{format}.expected.{extension}"
            ));
        }

        for path in paths {
            let file = checked(analyze(&corpus::read(&path), &path, &options()));

            let procedures = modules(&file).iter().flat_map(|module| &module.procedures);
            let found: Vec<_> = procedures
                .filter(|procedure| suspected(&procedure.traits))
                .map(|procedure| procedure.name.as_str())
                .collect();
            assert_eq!(found, [name], "{path}");
            assert!(suspected(&file.traits), "{path}");
        }
    }
}
