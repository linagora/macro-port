# Macro-Port: Lot 1 specification (Inventory)

Status: draft for implementation · Owner: Michel-Marie Maudet (LINAGORA) · Date: 2026-10-18 · Revision 2 (after technical review)

## 1. Goal

Lot 1 delivers a deterministic, offline inventory of VBA macros in Excel files. It answers three questions for every workbook and every procedure: what code is there, what is it used for (usage family), and how risky or complex it is. It ships as a single CLI binary (Linux, Windows, macOS) and as a browser mini web app powered by the same core compiled to WebAssembly.

Lot 1 contains **no AI and no code conversion**. Migration targets (OnlyOffice, Grist, LibreOffice, retire, keep on Excel) are a Lot 2 concern; the report reserves a field for them but Lot 1 never fills it.

## 2. Non-goals (Lot 1)

- No LLM calls, no code translation, no compatibility scoring against target suites.
- No execution of any macro, formula, script or embedded object, ever.
- No network access, no telemetry, no update checks.
- No p-code decompilation (detection of missing source is in scope, decompiling is not).
- No decryption of password-encrypted files.
- No Word or PowerPoint files (the container layer must not prevent adding them later).
- No editing or writing of the analysed files.

## 3. Principles

1. **Pure core.** `macroport-core` takes bytes and returns a `Report` value. It performs no file system, network, clock or environment access. Timestamps, durations and paths are passed in by the caller. This is what makes the WASM build possible and the output reproducible.
2. **Hostile input.** Every file is assumed malicious or corrupted. Parsers never panic on input-dependent paths, enforce size and recursion limits, and return a typed error with a file status. Every string coming from a file (names, literals, paths, references) is untrusted when rendered (section 8).
3. **Deterministic output.** Same input bytes + same rules version + same caller-injected values = byte-identical JSON (stable ordering, documented tie-breaks).
4. **Rules as data.** Usage families, security signals, complexity thresholds and traits live in YAML under `rules/`, embedded at build time and overridable with `--rules` in the CLI. No family logic hard-coded in Rust beyond the generic matchers and the closed list of structural predicates (section 7).
5. **Privacy by default.** Reports can be produced in redacted mode for sharing outside the organisation (section 8).

## 4. Architecture

Cargo workspace:

| Crate / folder | Role |
| --- | --- |
| `crates/macroport-core` | Containers, VBA extraction, lexer, structural parser, metrics, rule engine, report model. `#![forbid(unsafe_code)]`. No I/O. |
| `crates/macroport-cli` | `macro-port` binary: directory walking, parallelism, output files, HTML and CSV rendering, i18n. |
| `crates/macroport-wasm` | `wasm-bindgen` bindings (section M5). |
| `web/` | Mini web app (TypeScript + Vite, Preact) consuming the WASM module. Static, offline-capable. |
| `rules/` | `families.yaml`, `security.yaml`, `complexity.yaml`. |
| `schema/` | `report.schema.json` (JSON Schema 2020-12, the public contract), `rules/*.schema.json` (written in the first PR, see section 7). |
| `i18n/` | Fluent files `en/*.ftl`, `fr/*.ftl`. |
| `corpus/` | Test corpus (text samples, binary fixtures, ground truth). |
| `docs/` | Specs, ADRs, acceptance criteria. |

Dependency guidance (final choice in ADR-0002, written by the implementer):

- `cfb` for Compound File Binary; `zip` with `default-features = false, features = ["deflate"]` (no C codecs: they break `wasm32-unknown-unknown` and widen the attack surface); `quick-xml` for OOXML parts.
- Code pages: `codepage` (numeric code page to `encoding_rs::Encoding`) plus `encoding_rs`. Code page 1200 (UTF-16LE) and unknown values must be handled explicitly.
- YAML: a maintained YAML 1.2 parser with serde support (e.g. `serde-saphyr` on top of `saphyr`, or `serde_norway`); `serde_yaml` is archived. Document the choice.
- `serde` / `serde_json`, `sha2`, `hmac`, `clap`, `walkdir`, `rayon`, `thiserror`, `fluent-bundle`, `wasm-bindgen`.
- Tests: `insta`, `jsonschema` with `default-features = false` (its default `resolve-http` feature pulls an HTTP client, forbidden even in dev-dependencies), `cargo-fuzz`, Playwright for the web app.

## 5. Milestones

Each milestone ends with one or more pull requests reviewed by the owner. Do not start a milestone before the previous one is merged, with one exception: while the binary fixtures of M1 are not available (see CORPUS-PLAN), M2 and M3 may be done first on the text corpus through the text entry point below. M5 always comes last.

### M0. Workspace, CI, rules loading (first PR)

Cargo workspace skeleton, CI (section 9), `rules/*.schema.json` for the three rules files, `macro-port rules check`, JSON Schema validation helper for tests, empty report generation validated against `schema/report.schema.json`.

### Text entry point (used by M2 and M3, and by the CLI for text modules)

Besides `analyze(bytes, file_name)`, the core exposes `analyze_modules(modules: Vec<ModuleSource>, file_name)` where `ModuleSource` = name, type and source text. It skips the container layer. A text file (`.bas`, `.cls`, `.frm`) is analysed as a file with `container: "text_module"` and a single module; `size` and `sha256` are those of the text file. Module type from the extension: `.bas` standard; `.frm` form; `.cls` document when it contains `Workbook_*` or `Worksheet_*` procedures or an `Attribute VB_Base` line, class otherwise.

**Line numbering (applies to every evidence `line`)**: lines are 1-based in the module source **as the file presents it**, with no renumbering:

- binary modules: the decompressed source, in which Excel puts `Attribute VB_*` lines at the top; those lines count;
- text modules: the physical lines of the file, including export headers (`VERSION`, `BEGIN`...`END`, form designer `Begin {GUID}`...`End` blocks, `Attribute` lines) and the corpus header block.

Lines that are not code are skipped by the parser but still counted: export headers, module-level and procedure-level `Attribute` lines (e.g. `Attribute Foo.VB_Description = "..."`), and the **corpus header block**: consecutive lines at the top of the file matching `^' (Source|Author|License|Retrieved|Family|Synthetic):` followed by one line consisting of a single `'`.

### M1. Containers and VBA extraction

| Format | Container | VBA location | Notes |
| --- | --- | --- | --- |
| .xlsm, .xltm, .xlam | OOXML zip | `xl/vbaProject.bin` (CFB), located through `[Content_Types].xml` / relationships, not by name only | |
| .xlsb | OOXML zip with binary parts | same | macro sheets are `.bin` parts |
| .xls, .xlt, .xla | CFB (BIFF8) | storage `_VBA_PROJECT_CUR` | |
| .xlsx, .xltx | OOXML zip | must not contain VBA | a VBA part triggers `sec.vbaproject-in-xlsx` |
| Encrypted OOXML | CFB with `EncryptionInfo` + `EncryptedPackage` | not reachable | status `encrypted`; if the encryption is the well-known default password (`VelvetSweatshop`) case, add note `encrypted_default_password_possible`. No decryption in Lot 1. |
| Encrypted .xls (FILEPASS record) | CFB | `_VBA_PROJECT_CUR` is **not** encrypted | flag `workbook_encrypted: true`, status `ok`, extract VBA normally |

**MS-OVBA extraction**, following [MS-OVBA] (Office VBA File Format Structure):

1. Read the CFB `VBA` storage: streams `dir`, `_VBA_PROJECT`, and one stream per module. CFB names compare case-insensitively.
2. Decompress the `dir` stream (MS-OVBA compression, section 2.4.1). Parse records in order, tolerating known quirks:
   - `PROJECTVERSION` (0x0009) declares a size of 4 but is followed by 6 bytes (4-byte major + 2-byte minor);
   - `PROJECTCOMPATVERSION` (0x004A) is optional and may precede `PROJECTCODEPAGE`;
   - unknown record ids: skip using their declared size when plausible, otherwise stop with `corrupted`.
3. For each module record: `MODULENAME`, `MODULENAMEUNICODE`, `MODULESTREAMNAME` + `MODULESTREAMNAMEUNICODE` (use the Unicode stream name to locate the stream when present), `MODULEOFFSET` (TextOffset), `MODULETYPE`.
4. Module type: `MODULETYPE` 0x0021 = procedural (standard); 0x0022 = document, class or designer. Disambiguate 0x0022 with the `PROJECT` stream lines: `Document=<name>/&H...` (document), `Class=<name>` (class), `BaseClass=<name>` (form / designer), `Module=<name>` (standard).
5. Decompress each module stream starting at `TextOffset`. Decode with `PROJECTCODEPAGE` (via `codepage` + `encoding_rs`), output UTF-8, normalise line endings to `\n`.
6. Protection: the `PROJECT` stream always contains `ID`, `CMG`, `DPB` and `GC` entries, even for unprotected projects (their values are encrypted per MS-OVBA 2.4.3). `vba_project_protected` = the decrypted `GC` (ProjectVisibilityState) equals 0x00 (not visible). Implement the data decryption of 2.4.3 for `GC` only; do not attempt password recovery. Protection does not encrypt module source: extract it.
7. References: `REFERENCEREGISTERED`, `REFERENCEPROJECT`, `REFERENCECONTROL` names (e.g. "Microsoft Outlook 16.0 Object Library"). Keep the display name; drop file system paths contained in the libid string (they may contain user names).
8. `source_missing_suspected`: module stream present, decompressed source empty or containing only `Attribute` lines, while the module's compiled part (bytes before `TextOffset`) is non-trivial (more than 512 bytes). Emit container indicator `sec.source-missing`.

**Excel 4.0 (XLM) macro sheets** (presence and count only): OOXML, any part with content type `application/vnd.ms-excel.macrosheet+xml` or `application/vnd.ms-excel.macrosheet` (binary, in .xlsb), or relationship type ending in `/xlMacrosheet`; BIFF8, `BoundSheet8` records with `dt = 1` (macro sheet).

**File statuses**:

| Situation | Status |
| --- | --- |
| VBA with at least one procedure, or XLM macro sheets present | `ok` |
| VBA project with only empty document modules (no procedure) and no XLM | `no_macros` (the project is still described) |
| No VBA project and no XLM | `no_macros` |
| OOXML encrypted package | `encrypted` |
| Above size limit | `too_large` |
| Compression anomaly (see limits) | `suspicious_archive` |
| Not a recognised container | `unsupported` |
| Recognised container, broken structure | `corrupted` |
| Unexpected internal error (should not happen; tested) | `error` |

**Limits** (defaults, configurable in the CLI): max input file size 200 MB (`too_large`). Inflate only the zip entries needed: `[Content_Types].xml`, `_rels/.rels`, `xl/_rels/workbook.xml.rels`, `xl/workbook.xml` (or `.bin`), the VBA part, and macro-sheet parts listed by content type. Read each through a bounded reader: 64 MB for the VBA part, 16 MB for XML parts. If an entry exceeds its cap while inflating, or if bytes actually inflated / compressed bytes read exceeds 1000:1 for that entry, stop with `suspicious_archive`. Declared sizes in zip headers are never trusted. CFB: max 2,000 directory entries, max storage depth 32, max stream size 64 MB, FAT chain loops detected. MS-OVBA: max decompressed `dir` stream 16 MB, max 2,000 modules. These limits keep peak memory per file under 256 MB.

Done when: A1 and A2 pass.

### M2. Lexer, structural parser, metrics

A full VBA grammar is not required. Implement:

1. **Logical lines**: join `_` continuations (a line ending with space + `_`), split `:` statement separators outside strings (but not label definitions such as `ErrHandler:` at line start), strip comments (`'` and `Rem` at statement start) while keeping comment text for the `comment` matcher.
2. **Lexer**: identifiers (including `[bracketed names]`), keywords (case-insensitive), string literals with `""` escapes, numbers (incl. `&H`, `&O`, type suffixes), dates `#...#`, operators, `.` and `!` member access, type-declaration characters (`$ % & ! # @`) on identifiers (`Chr$` = `Chr`).
3. **Structural parse**: module-level declarations (`Option`, `Declare`, `Const`, variables, `Type`, `Enum`, `Implements`, `WithEvents`, `#If`/`#Const`); procedures (`Sub`, `Function`, `Property Get/Let/Set`, visibility, `Static`, parameters, line span); inside procedures: call sites, member chains (e.g. `Workbooks.Open`, `ThisWorkbook.Worksheets("X").Range("A1").Interior.Color`), `With` block resolution (`.Font` inside `With Range(...)` gives chain `Range.Font`; nested `With` supported), `CreateObject` / `GetObject` ProgIDs.
4. **Constant folding** (the only form of evaluation allowed, closed set): string literals joined with `&` or `+`; `Chr`, `ChrW`, `ChrB` (and `$` variants) of a numeric literal; `StrReverse`, `LCase`, `UCase` of a foldable expression; references to module-level `Const` strings. Used for ProgIDs and for `string` matchers. Anything else is left unresolved.
5. **Call graph** inside the project: which procedure calls which, by name, best effort, including `Application.Run "Name"` and `CallByName obj, "Name"` when the name folds to a literal. Entry points: event handlers, `auto_exec` procedures, public procedures of standard modules without parameters (they may be bound to buttons or shortcuts), public functions of standard modules (possible UDFs). `possibly_unused` = not an entry point and never referenced.
6. **Error recovery**: unbalanced blocks or unparseable statements never abort the module; the procedure gets trait `compile_error_suspected` (predicate `parse_recovered`).
7. **Metrics** per procedure:
   - `loc`: physical lines from `Sub`/`Function` to `End`; `logical_loc`: logical lines that are neither blank nor comment-only;
   - `cyclomatic` = 1 + number of decision points, counted on tokens: each `If` and `ElseIf` (single-line `If ... Then ... Else ...` counts 1); each `Case` clause except `Case Else` (a clause `Case 1, 2, 3` counts 1); each loop statement once (`For`, `For Each`, `Do` (whether the condition is on `Do While/Until` or `Loop While/Until`), `While`...`Wend`); each `And`, `Or`, `Xor` operator inside an `If`/`ElseIf`/`Do`/`Loop`/`While` condition; each `On Error GoTo <label>` statement whose label is not `0`; `IIf` calls count 1 each.
   - `max_nesting`: deepest nesting of `If` (block form), `Select Case`, `For`, `Do`, `While`, `With`;
   - `parameters`, `goto_count` (`GoTo` and `GoSub`, excluding `On Error GoTo`).
   Module and file aggregates: `loc` and `logical_loc` summed; `cyclomatic` = max; `cyclomatic_total` = sum; `max_nesting` = max; `procedures`, `forms`, `declare_count`, `external_dependencies` as defined in `rules/complexity.yaml`.

Done when: insta snapshots over all `corpus/families/**` are stable and reviewed; no panic on any corpus file, including the ones with deliberate compile errors (01-06, 02-04, 05-04, 06-02, 08-04, 08-05, 09-03).

### M3. Rule engine: families, security, complexity

- **Families**: each procedure gets a score per family (sum of matched signal weights, each signal capped by `max_hits_per_procedure`; module- and project-scope signals contribute half their weight to every procedure of the module or project). `primary_family`: highest score at or above `min_score`, else `unclassified`. **Tie-break**: higher score, then more distinct matched signals, then family id ascending. `secondary_families`: score at or above `secondary_ratio` × primary score. Every procedure-level family score carries evidence (signal id, line, snippet of at most 200 characters). File-level `families` = sum of procedure scores per family (no evidence required at file level); file `primary_family` uses the same tie-break.
- **Security signals**: per procedure (and module/project scope), with severity; chains evaluated per file. File `max_severity` = max over all signals and container indicators, `null` if none.
- **Complexity level** per file and **traits** per procedure and file, from `rules/complexity.yaml`.

Done when: A4 and A5 pass.

### M4. Reports and CLI

```
macro-port scan <path>... [-o out_dir] [--format json,html,csv] [--lang en|fr] [--rules dir] [--jobs N]
                          [--max-file-size MB] [--no-snippets] [--redact [--redact-key KEY] [--redact-identifiers]]
macro-port file <file> [--format json|html] [--lang en|fr]
macro-port rules check [--rules dir]
macro-port schema
```

- Files considered: extensions `xlsm xltm xlam xlsb xls xlt xla xlsx xltx bas cls frm` (case-insensitive). Recursive walk, symlinks not followed by default. Text modules go through the text entry point.
- Outputs in `out_dir`: `report.json` (fleet report, validates against the schema), `report.html` (single self-contained file), `files.csv`, `procedures.csv`, `signals.csv`.
- CSV columns: `files.csv` = path, container, status, complexity_level, primary_family, max_severity, modules, procedures, logical_loc, xlm_present, vba_project_protected; `procedures.csv` = path, module, procedure, kind, primary_family, secondary_families, logical_loc, cyclomatic, signals, traits; `signals.csv` = path, module, procedure, signal_id, severity, line. UTF-8 with BOM (for Excel), comma separator, RFC 4180 quoting.
- `generated_at` and `scan.duration_ms` are injected by the CLI (with `--deterministic`, `generated_at` = `1970-01-01T00:00:00Z` and `duration_ms` omitted, for tests).
- Exit codes: 0 success, 1 completed with at least one file in `corrupted`, `error`, `suspicious_archive` or `too_large`, 2 usage error, 3 fatal error.
- Parallel analysis with `rayon`, bounded by `--jobs`; progress on stderr only.
- i18n: all user-facing strings through Fluent (`en` default, `fr`); family and signal labels from the YAML `label` fields.

Done when: A3, A6, A7, A8 pass.

### M5. WASM module and mini web app

`macroport-wasm` exposes:

```ts
analyze(bytes: Uint8Array, fileName: string, options: { lang: 'en'|'fr', redact: boolean, redactKey?: string }): string // single-file report JSON (files.length === 1)
rulesVersion(): string
schemaVersion(): string
```

The web app aggregates single-file reports into a fleet report with the same structure as the CLI's.

Mini web app (`web/`), static, deployable on GitHub Pages (no custom headers available, so the policy is a `<meta http-equiv="Content-Security-Policy">`):

- CSP: `default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; style-src 'self'; img-src 'self' data:; connect-src 'none'; manifest-src 'self'; base-uri 'none'; form-action 'none'`. The page fetches nothing, not even from its own origin: the `.wasm` is embedded in the JavaScript bundle and compiled from those bytes.
- The analysis runs in a dedicated Web Worker created from a same-origin script; the worker receives the WASM module from the page (compiled `WebAssembly.Module` posted to the worker) so that it performs no fetch of its own.
- Offline: a service worker precaches the app shell, whose JavaScript bundle holds the `.wasm`, on first load.
- Features: drag and drop of files and folders (`webkitdirectory`), sequential processing with a progress bar; fleet view (totals, by complexity, by family, by severity); file view (modules, procedures, metrics, signals, evidence, source viewer with highlighted lines); filters and search; export JSON and CSV (same formats as the CLI); EN/FR switch; visible notice "Your files never leave this browser".
- The app contains no analysis logic: it only renders report JSON from the core.
- The end-to-end network monitor in A9 is the authoritative check that no request leaves the browser: a `<meta>` policy does not apply to the workers.

Done when: A9 passes.

## 6. Report contract

`schema/report.schema.json` is the source of truth; `schema/examples/report.example.json` is a valid example and must stay consistent with the rules (a test re-derives it, see A3). Any change requires bumping `schema_version` (additive = minor, breaking = major) and an ADR. `orientation` is reserved for Lot 2 and is `null` or absent in Lot 1.

## 7. Rule format

Rules are YAML 1.2 files. In the first PR (M0), write `schema/rules/families.schema.json`, `security.schema.json` and `complexity.schema.json` that accept exactly the keys documented here and used in `rules/*.yaml`; `macro-port rules check` validates against them, compiles every regex, and rejects duplicate ids. Unknown keys are errors. Avoid YAML 1.1 boolean-like keys (`on`, `yes`, `no`).

**Matchers** (identifiers are compared case-insensitively; **all regexes are case-insensitive by default**; type-declaration characters are ignored, `Chr$` = `Chr`):

| Matcher | Matches | Example |
| --- | --- | --- |
| `member` | an identifier used as a member (`.X`, `!X`) or as a standalone object name; never a named argument (`X:=`) | `[Interior, Borders]` |
| `chain` | a member chain suffix, dot-separated, after `With` resolution; intermediate call arguments are ignored (`Worksheets(1).Range("A1").Interior` matches `Range.Interior`) | `Workbooks.Open` |
| `call` | a procedure, function or statement invoked by name, not preceded by `.` | `[Shell, Dir, InputBox]` |
| `progid` | ProgID passed to `CreateObject` / `GetObject`, after constant folding; glob patterns | `Outlook.Application` |
| `procedure_name` | regex on procedure names | `^Worksheet_\w+$` |
| `module_type` | module type (`standard`, `class`, `document`, `form`) | `form` |
| `reference` | regex on VBA project reference display names | `Microsoft Outlook` |
| `regex` | regex on logical code lines; strings and comments blanked unless `text: raw` | `\bOpen\s+.+\s+For\s+Output\b` |
| `string` | regex on string literal contents (after constant folding when foldable) | `password\s*=` |
| `comment` | regex on comment text | `macro (recorded\|enregistr)` |
| `structural` | named predicate implemented in Rust, closed list below | `udf_candidate` |

**Structural predicates** (closed list for Lot 1):

- `udf_candidate`: `Public` or default-visibility `Function` in a standard module, whose body has no workbook side effect: no assignment to `.Value`, `.Formula`, `.Interior`, `.Font` members, no `Select`, `Activate`, `Copy`, `Delete`, `Insert`, `ClearContents`, `MsgBox`.
- `event_handler`: procedure named `<object>_<event>` in a document module (`Workbook_*`, `Worksheet_*`, `Chart_*`), in a class or document module where `<object>` is a `WithEvents` variable, or in a form module where `<object>` is `UserForm` or a control name.
- `auto_exec`: procedure named `Auto_Open`, `Auto_Close`, `AutoOpen`, `AutoClose`, `AutoExec`, or `Workbook_Open`, `Workbook_Activate`, `Workbook_BeforeClose` in a document module. (Auto-exec procedures are entry points; `is_event_handler` is true only for `event_handler` matches.)
- `chr_obfuscation`: 3 or more `Chr`/`ChrW`/`ChrB` calls in one expression joined by `&`/`+`, or `StrReverse` applied to a literal.
- `long_hex_or_base64_literal`: string literal of 200 or more characters entirely in the hex or base64 alphabet.
- `nested_loop_compare`: a loop nested inside another loop whose body contains an `If`/`ElseIf` comparing values with `=`, `<>` or `StrComp`.
- `possibly_unused`, `parse_recovered`: computed by M2, used by traits.

**Signal keys**: `id`, `match`, `pattern` (string or list), `weight` (families), `severity` and `category` (security), `label` (`en`, `fr`), `max_hits_per_procedure` (default from `scoring.default_max_hits_per_procedure`), `scope` (`procedure` default, `module`, `project`), `text` (`code` default or `raw`), `requires_family` (security only: reported only when the procedure's primary or a secondary family equals it), `note` (ignored). Glob `*` in a family `progid` pattern matches any ProgID not matched by another family signal; in security it matches any ProgID.

**File-level keys**: `families.yaml`: `version`, `scoring` (`min_score`, `secondary_ratio`, `default_max_hits_per_procedure`), `families[]` (`id`, `corpus_slug`, `label`, `signals[]`). `security.yaml`: `version`, `signals[]`, `chains[]` (`id`, `all` = list of terms, a term being a signal id or `{ any: [terms] }`, `severity`, `label`), `container_indicators[]` (`id`, `severity`, `label`). `complexity.yaml`: `version`, `file_levels` (`medium`, `high`, each `any:` list of `{ metric, gte | lte }`), `metric_definitions` (documentation), `traits[]` (`id`, `label`, `description`, and one of `any:` / `all:` lists of conditions or `structural:`). Trait conditions: `{ metric, gte | lte }`, `{ match: comment | procedure_name, pattern }`, `{ match: ratio, of: [members or chains], per_logical_loc_gte: x }` (number of occurrences of the listed members or chains divided by the procedure's `logical_loc`).

## 8. Output safety and privacy

**Escaping.** Every string from an analysed file (paths, module and procedure names, snippets, references, error messages) is untrusted:

- HTML report and web app: text inserted through text nodes or framework escaping only; no `innerHTML`, no `dangerouslySetInnerHTML`, no string-built HTML. The HTML report embeds its data as JSON in a `<script type="application/json">` block with `<`, `>`, `&`, U+2028, U+2029 escaped as `<` etc., and carries a meta CSP `default-src 'none'; script-src 'sha256-...'; style-src 'sha256-...'; img-src data:` with hashes computed at build time.
- CSV: any cell starting with `=`, `+`, `-`, `@`, tab or carriage return is prefixed with `'` (formula-injection protection).
- Tests include a fixture whose module name, procedure name and a string literal contain `<img src=x onerror=alert(1)>` and `=HYPERLINK("http://x")`.

**Redaction** (`--redact`): a keyed HMAC-SHA256 replaces sensitive values. Key = `--redact-key` (deterministic across runs) or a random key per run (unlinkable). Hash output = first 16 hex characters.

| Field | Redacted form |
| --- | --- |
| `files[].path`, `scan.roots[]` | `hmac(path)` + original extension |
| `snippet` (all evidence) | removed |
| `errors[].message` | removed (code kept) |
| `vba_project.name` | `hmac(name)` |
| `references[]` | kept (library display names), paths already stripped |
| module and procedure names | kept by default (needed for review); hashed with `--redact-identifiers` |
| metrics, families, signals, traits, statuses | kept |

## 9. Quality gates (every PR)

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace`, `cargo deny check` (licences compatible with Apache-2.0, advisories, ban list including HTTP client crates such as `reqwest`, `hyper`, `ureq`, `isahc`, `curl`), JSON outputs validated against the schema, `wasm-pack build --target web` succeeds, `npm run build` and `npm test` in `web/`. Fuzz targets for zip entry reading, CFB, `dir` stream parsing, MS-OVBA decompression and the lexer run in nightly CI.
