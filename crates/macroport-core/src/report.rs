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

use crate::rules::{ModuleType, Severity};

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
