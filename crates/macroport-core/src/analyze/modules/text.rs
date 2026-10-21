// SPDX-License-Identifier: Apache-2.0
//! Modules handed over as text (SPEC, "Text entry point"). [`analyze_modules`]
//! skips the container layer, so it reads no file: it gives a file of
//! `container: "text_module"` whose VBA project holds the modules, and the
//! front end reads them like those of a workbook.

use sha2::{Digest, Sha256};

use super::finish;
use crate::analyze::project::Project;
use crate::analyze::{Extraction, Options, Reading, hex, sha256_hex, too_large};
use crate::report::{Container, FileReport, Metrics, ModuleReport, VbaProject};
use crate::rules::ModuleType;
use crate::vba::ModuleSource;

/// The most bytes that the texts handed to [`analyze_modules`] may total: as
/// much as the sources of a VBA project may total once decoded
/// (`MAX_PROJECT_SOURCE` in `project.rs`). The limit of
/// [`Options::max_file_size`] applies as well when it is lower.
const MAX_TEXT_SIZE: u64 = 32 * 1024 * 1024;

/// Analyses VBA modules that the caller hands over as text: a file of
/// `container: "text_module"` whose VBA project holds them (SPEC, "Text entry
/// point"). It skips the container layer, and so reads no file.
///
/// `path` is the path as the report gives it. The `size` of the file is the
/// bytes of the texts and its `sha256` the hash of the texts one after the
/// other, which for one module is its text. Texts above 32 MiB in all, or above
/// [`Options::max_file_size`] when that is lower, are `too_large` and not
/// read. A module that cannot be read gets the failure that
/// [`analyze`](crate::analyze::analyze) gives, and the other modules are read
/// all the same. The result never fails.
pub fn analyze_modules(
    modules: Vec<ModuleSource>,
    path: &str,
    options: &Options<'_>,
) -> FileReport {
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    for module in &modules {
        hasher.update(module.text.as_bytes());
        size = size.saturating_add(u64::try_from(module.text.len()).unwrap_or(u64::MAX));
    }
    let sha256 = hex(&hasher.finalize());
    if size > text_limit(options) {
        return too_large_text(path, size, sha256, options);
    }
    let (reports, sources): (Vec<_>, Vec<_>) = modules
        .into_iter()
        .map(|module| {
            let report = text_module(&module.name, module.module_type, &module.text);
            (report, Some(module.text))
        })
        .unzip();
    finish(
        text_extraction(path, size, sha256, reports, sources),
        options,
    )
}

/// The limit on the size of a text: that of the options, and [`MAX_TEXT_SIZE`].
fn text_limit(options: &Options<'_>) -> u64 {
    options.max_file_size.min(MAX_TEXT_SIZE)
}

/// The report of a text above the size limit. Like any file above it, it is
/// not read.
fn too_large_text(path: &str, size: u64, sha256: String, options: &Options<'_>) -> FileReport {
    let limited = Options {
        max_file_size: text_limit(options),
        ..*options
    };
    let mut file = too_large(path, size, sha256, &limited);
    file.container = Container::TextModule;
    file
}

/// The extraction of text modules: their reports, which the front end has not
/// filled, and their texts.
fn text_extraction(
    path: &str,
    size: u64,
    sha256: String,
    modules: Vec<ModuleReport>,
    sources: Vec<Option<String>>,
) -> Extraction {
    let report = VbaProject {
        name: None,
        protected: false,
        code_page: None,
        references: None,
        modules,
    };
    let mut reading = Reading::new(Container::TextModule);
    reading.project = Some(Project { report, sources });
    reading.into_extraction(path, size, sha256)
}

/// The report of a text module, as extraction would give it before the front
/// end reads it. A text has no compiled part, so its source is never suspected
/// to be missing.
fn text_module(name: &str, module_type: ModuleType, text: &str) -> ModuleReport {
    ModuleReport {
        name: name.to_owned(),
        module_type,
        stream_name: None,
        source_available: true,
        source_missing_suspected: None,
        source_sha256: Some(sha256_hex(text.as_bytes())),
        metrics: Metrics::default(),
        declarations: None,
        procedures: Vec::new(),
        signals: None,
    }
}
