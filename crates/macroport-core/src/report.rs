// SPDX-License-Identifier: Apache-2.0
//! The report: the model of `schema/report.schema.json`, the public contract.
//!
//! Each struct mirrors an object of the schema, its fields in the order of the
//! schema's keys, which is the order serialisation writes them in. A key that
//! the schema requires is a plain field; an optional key is an `Option`, left
//! out when `None`, except the two keys that may be null: `vba_project` and
//! `max_severity` are always written, `None` as null. `orientation` is always
//! null in Lot 1. Maps are `BTreeMap`s, so their keys come out in one order
//! whatever the order they went in: alphabetical for strings, the order of the
//! variants for enums, which follows the schema. Every struct denies unknown
//! keys, so reading a report checks that the model knows each of its keys.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};

use crate::rules::{ModuleType, RuleSet, Severity};

/// The `schema_version` of the reports that this model writes.
pub const SCHEMA_VERSION: &str = "0.1.0";

/// A report on one or many Excel files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: String,
    pub tool: Tool,
    /// An RFC 3339 date-time, injected by the caller, never by the core.
    pub generated_at: String,
    /// True when snippets are removed and paths hashed.
    pub redacted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<Lang>,
    pub scan: Scan,
    pub files: Vec<FileReport>,
    pub summary: Summary,
}

impl Report {
    /// The fleet report on zero files. `generated_at` is an RFC 3339
    /// date-time that the caller reads from its clock or fixes for tests;
    /// `tool.build` is left for the caller to set.
    pub fn empty(rules: &RuleSet, generated_at: String, lang: Lang) -> Self {
        Self::from_files(rules, generated_at, lang, Vec::new())
    }

    /// The fleet report on `files`, the reports of the files of a scan in any
    /// order.
    ///
    /// The files come out in the order of their paths, byte by byte, so that
    /// the report does not depend on the order in which they were analysed.
    /// Two files of the same path, which a name that is not UTF-8 can give,
    /// come out in the order of their hashes. A caller that rewrites the paths
    /// afterwards sorts the files again; [`crate::redact::Redaction::apply`]
    /// does.
    ///
    /// The counts of the scan and the summary follow from the files.
    /// `generated_at` is an RFC 3339 date-time that the caller reads from its
    /// clock or fixes for tests. `scan.roots`, `scan.duration_ms` and
    /// `tool.build` are left for the caller to set: the files do not tell them.
    pub fn from_files(
        rules: &RuleSet,
        generated_at: String,
        lang: Lang,
        mut files: Vec<FileReport>,
    ) -> Self {
        Self::sort_files(&mut files);
        Self {
            schema_version: SCHEMA_VERSION.to_owned(),
            tool: Tool {
                name: "macro-port".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                rules_version: rules.version().to_owned(),
                build: None,
            },
            generated_at,
            redacted: false,
            lang: Some(lang),
            scan: Scan::of(&files),
            summary: Summary::of(&files),
            files,
        }
    }

    /// Puts `files` in the order of a report: by path, byte by byte, then by
    /// hash.
    pub(crate) fn sort_files(files: &mut [FileReport]) {
        files.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.sha256.cmp(&b.sha256)));
    }

    /// The report as the text of a JSON file: indented by two spaces like the
    /// example report of the schema, and ending with a newline.
    ///
    /// # Errors
    ///
    /// Those of `serde_json`, which this model does not raise: its map keys
    /// are strings or unit variants, and no field has a custom serialiser
    /// that fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }
}

/// The tool that wrote the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// Always `macro-port`.
    pub name: String,
    pub version: String,
    /// The version that the three rules files share.
    pub rules_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<Build>,
}

/// The build of the tool that wrote the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Build {
    Cli,
    Wasm,
}

/// The language of the user-facing strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    En,
    Fr,
}

/// What the scan covered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Vec<String>>,
    pub files_total: u32,
    pub files_with_macros: u32,
    pub files_failed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl Scan {
    /// The counts of a scan that produced `files`. The caller sets the roots
    /// and the duration.
    ///
    /// `files_with_macros` counts the files in status `ok`: those with a VBA
    /// procedure or an Excel 4.0 macro sheet (SPEC M1, file statuses).
    /// `files_failed` counts those in a failure status, see
    /// [`Status::is_failure`].
    fn of(files: &[FileReport]) -> Self {
        let counted =
            |keep: fn(Status) -> bool| saturating_count(files.iter().filter(|f| keep(f.status)));
        Self {
            roots: None,
            files_total: saturating_count(files.iter()),
            files_with_macros: counted(|status| status == Status::Ok),
            files_failed: counted(Status::is_failure),
            duration_ms: None,
        }
    }
}

/// One analysed file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReport {
    /// Relative path, or hashed path when redacted.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub container: Container,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FileError>>,
    /// Null when no VBA project was read.
    pub vba_project: Option<VbaProject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xlm: Option<Xlm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity_level: Option<Level>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_family: Option<String>,
    /// Aggregated over procedures (sum of scores).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub families: Option<Vec<FamilyScore>>,
    /// The highest severity of the file's signals, null when it has none.
    pub max_severity: Option<Severity>,
    /// File-scope and chained signals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signals: Option<Vec<Signal>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traits: Option<Vec<String>>,
    #[serde(default)]
    pub orientation: Orientation,
    /// BIFF8 FILEPASS present: workbook stream encrypted, VBA still extracted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workbook_encrypted: Option<bool>,
    /// Machine-readable notes, e.g. `encrypted_default_password_possible`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
}

/// The container format of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Xlsm,
    Xltm,
    Xlam,
    Xlsb,
    Xls,
    Xlt,
    Xla,
    Xlsx,
    Xltx,
    /// A VBA module exported as text (`.bas`, `.cls`, `.frm`).
    TextModule,
    Unknown,
}

/// The outcome of a file's analysis. The order of the variants, that of the
/// schema, is the order of the keys of `summary.by_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    NoMacros,
    Encrypted,
    Corrupted,
    Unsupported,
    TooLarge,
    SuspiciousArchive,
    Error,
}

impl Status {
    /// Whether a file in this status failed: it is damaged, above the size
    /// limit, an archive that inflates suspiciously, or the analysis broke on
    /// it. These are the files that make `macro-port scan` exit with code 1
    /// (SPEC M4), and the ones that `scan.files_failed` counts.
    ///
    /// A file that is encrypted or in a format the tool does not read did not
    /// fail: it is a finding of the inventory.
    pub fn is_failure(self) -> bool {
        // No wildcard: a new status has to be sorted into one side or the other.
        match self {
            Self::Corrupted | Self::TooLarge | Self::SuspiciousArchive | Self::Error => true,
            Self::Ok | Self::NoMacros | Self::Encrypted | Self::Unsupported => false,
        }
    }
}

/// An error met while analysing a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileError {
    pub code: String,
    pub message: String,
}

/// The VBA project of a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VbaProject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_page: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<String>>,
    pub modules: Vec<ModuleReport>,
}

/// The Excel 4.0 (XLM) macros of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Xlm {
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macro_sheets: Option<u32>,
}

/// The complexity level of a file, ordered from `Low` to `High` like the keys
/// of `summary.by_complexity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    Medium,
    High,
}

/// One VBA module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleReport {
    pub name: String,
    #[serde(rename = "type")]
    pub module_type: ModuleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,
    pub source_available: bool,
    /// Stream present, source empty or truncated, p-code present (VBA
    /// stomping indicator).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_missing_suspected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    pub metrics: Metrics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declarations: Option<Declarations>,
    pub procedures: Vec<ProcedureReport>,
    /// Module-scope signals (e.g. `Declare` statements).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signals: Option<Vec<Signal>>,
}

/// What the declarations section of a module holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declare_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_events_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_explicit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_vba_support: Option<bool>,
}

/// One procedure of a module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcedureReport {
    pub name: String,
    pub kind: ProcedureKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_entry_point: Option<bool>,
    /// True only for the `event_handler` predicate (auto-exec procedures are
    /// entry points but not event handlers unless they are document-module
    /// events).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_event_handler: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub possibly_unused: Option<bool>,
    pub start_line: u32,
    pub end_line: u32,
    pub metrics: Metrics,
    /// Procedures of the same project called by this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls: Option<Vec<String>>,
    /// Object model members, chains, functions and ProgIDs used, normalised
    /// (e.g. `Range.Interior`, `CreateObject:Scripting.Dictionary`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apis: Option<Vec<ApiUse>>,
    pub primary_family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secondary_families: Option<Vec<String>>,
    pub families: Vec<FamilyScore>,
    pub signals: Vec<Signal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traits: Option<Vec<String>>,
    #[serde(default)]
    pub orientation: Orientation,
}

/// How a procedure is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureKind {
    Sub,
    Function,
    PropertyGet,
    PropertyLet,
    PropertySet,
}

/// The access modifier of a procedure, `Default` when it declares none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Friend,
    Default,
}

/// How many times a procedure uses an API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiUse {
    pub name: String,
    pub count: u32,
}

/// The migration target, reserved for Lot 2: always null in Lot 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orientation;

/// The size and complexity metrics of a procedure, a module or a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub loc: u32,
    pub logical_loc: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedures: Option<u32>,
    /// Per procedure: value; per module or file: maximum.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclomatic: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cyclomatic_total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_nesting: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goto_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declare_count: Option<u32>,
    /// See `rules/complexity.yaml`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_dependencies: Option<u32>,
}

/// The score of a usage family for a procedure or a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyScore {
    /// Family id from `rules/families.yaml`, e.g. `04-lookup`, or
    /// `unclassified`.
    pub family: String,
    #[serde(serialize_with = "write_score")]
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<Evidence>>,
}

/// Writes a whole score as an integer, as the example report does (`3`, not
/// `3.0`), and any other score as a float.
fn write_score<S: Serializer>(score: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if score.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(score) {
        // Whole and in range: the cast is exact.
        serializer.serialize_u32(*score as u32)
    } else {
        serializer.serialize_f64(*score)
    }
}

/// A security signal raised in a procedure, a module or a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub signal_id: String,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Vec<Evidence>>,
}

/// Where a rule matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub rule_id: String,
    /// 1-based line in the module source.
    pub line: u32,
    /// At most 200 characters, absent when redacted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Counts over the files of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub by_status: BTreeMap<Status, u32>,
    /// Number of procedures per primary family.
    pub by_family: BTreeMap<String, u32>,
    pub by_complexity: BTreeMap<Level, u32>,
    pub by_severity: BTreeMap<Severity, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totals: Option<Totals>,
}

impl Summary {
    /// The summary of `files`. Only the statuses, families, levels and
    /// severities that occur are counted.
    ///
    /// The files are counted by status, by complexity level and by highest
    /// severity; the procedures by primary family. The totals add up the
    /// modules and procedures of the VBA projects, the logical lines of the
    /// files and the procedures that are possibly unused.
    fn of(files: &[FileReport]) -> Self {
        let mut by_status = BTreeMap::new();
        let mut by_family = BTreeMap::new();
        let mut by_complexity = BTreeMap::new();
        let mut by_severity = BTreeMap::new();
        let mut modules = 0_u32;
        let mut procedures = 0_u32;
        let mut logical_loc = 0_u32;
        let mut possibly_unused = 0_u32;
        for file in files {
            tally(&mut by_status, file.status);
            if let Some(level) = file.complexity_level {
                tally(&mut by_complexity, level);
            }
            if let Some(severity) = file.max_severity {
                tally(&mut by_severity, severity);
            }
            if let Some(metrics) = &file.metrics {
                logical_loc = logical_loc.saturating_add(metrics.logical_loc);
            }
            let Some(project) = &file.vba_project else {
                continue;
            };
            modules = modules.saturating_add(saturating_count(project.modules.iter()));
            for procedure in project.modules.iter().flat_map(|m| &m.procedures) {
                procedures = procedures.saturating_add(1);
                tally(&mut by_family, procedure.primary_family.clone());
                if procedure.possibly_unused == Some(true) {
                    possibly_unused = possibly_unused.saturating_add(1);
                }
            }
        }
        Self {
            by_status,
            by_family,
            by_complexity,
            by_severity,
            totals: Some(Totals {
                modules: Some(modules),
                procedures: Some(procedures),
                logical_loc: Some(logical_loc),
                possibly_unused_procedures: Some(possibly_unused),
            }),
        }
    }
}

/// Counts one more occurrence of `key`.
fn tally<K: Ord>(counts: &mut BTreeMap<K, u32>, key: K) {
    let seen = counts.entry(key).or_insert(0);
    *seen = seen.saturating_add(1);
}

/// The number of `items`, at most `u32::MAX`.
fn saturating_count<I: Iterator>(items: I) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// Totals over the files of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Totals {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modules: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procedures: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_loc: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub possibly_unused_procedures: Option<u32>,
}
