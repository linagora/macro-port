# Macro-Port Lot 1: acceptance criteria

Every criterion is checked by an automated test in CI unless marked *manual*. Lot 1 is accepted when all criteria pass on the tagged release candidate.

Baseline: a deliberately naive regex prototype (`tools/rules-prototype/`) applying the v0.1.0 rules to the text corpus reaches 70 % top-1 and 88 % top-2 family accuracy on families 01 to 13, and detects 17 of 17 expected security signals on family 14. The Rust implementation, with a real parser and `With` resolution, must do better.

## A1. Extraction completeness

- For every binary fixture listed in `corpus/fixtures/fixtures.yaml`, the extracted module list (names, types) equals the expected list. The list includes the default document modules Excel creates (`ThisWorkbook`, the first sheet, localised names such as `Feuil1`), which the fixture script records.
- The extracted source of each module equals its `expected_source` file (exported by Excel when the fixture was built, see `tools/fixtures/Build-Fixtures.ps1`), after normalising line endings and trailing whitespace and ignoring `VERSION`, `BEGIN`/`END` and `Attribute VB_*` header lines. The corpus text itself is not the reference, because the VBA editor normalises identifier casing.
- Code pages: fixtures saved with code pages 1252, 1251 and 932 decode to the expected UTF-8 text.
- `vba_project_protected` is true for the password-protected fixtures, and their source is still extracted.
- Encrypted OOXML fixtures return status `encrypted`. Encrypted `.xls` fixtures (FILEPASS) return status `ok`, `workbook_encrypted: true`, and their VBA modules are extracted.
- XLM macro-sheet fixtures report `xlm.present = true` with the right sheet count.

## A2. Robustness

- No panic, no infinite loop, peak memory under 256 MB per file, on any file in `corpus/` and `corpus/fixtures/malformed/` (truncated zips, corrupted CFB headers and FAT chains, invalid `dir` streams, invalid compression chunks, zip bomb with ratio above 1000:1, deeply nested CFB storages, 0-byte files, random bytes renamed `.xlsm`).
- Each malformed file returns a status among `corrupted`, `unsupported`, `too_large`, `suspicious_archive`, `error`, with an error code.
- Fuzz targets (zip entry reading, CFB, `dir` stream parser, MS-OVBA decompression, lexer) run 1 hour each in nightly CI without crash.
- Text samples with deliberate compile errors (01-06, 02-04, 05-04, 06-02, 08-04, 08-05, 09-03) are analysed with trait `compile_error_suspected` where relevant and without failing the file.

## A3. Report contract

- 100 % of JSON outputs (CLI and WASM, redacted or not) validate against `schema/report.schema.json`.
- Output is deterministic: two runs with `--deterministic` on the same inputs produce byte-identical JSON, whatever `--jobs` is.
- A test re-derives `schema/examples/report.example.json` from fixture `14-01` (binary) and compares it field by field, so the example cannot drift from the rules.
- `orientation` is null or absent everywhere.

## A4. Family classification (text corpus, files of families 01 to 13)

Ground truth: `family` in `corpus/manifest.json` (74 included or extracted samples with a file; synthetic samples count).

- File-level top-1 accuracy (file `primary_family` equals ground truth) at least **75 %**.
- File-level top-2 accuracy (ground truth among the two highest-scoring families) at least **90 %**.
- Per family, top-2 recall at least **60 %**.
- Every procedure-level family score has at least one evidence item whose line number points inside the procedure (line numbering as defined in SPEC section 5).
- The confusion matrix is published in the CI artefacts and reviewed at each rules change.

Rules may be tuned to reach these numbers, but each tuning commit must state the before and after metrics in its description; tuning must not add sample-specific patterns (no matching on procedure names or literals that only appear in one corpus file).

## A5. Security signals

- All signals listed in `corpus/ground-truth/security-expected.yaml` are detected (currently 17 for 6 samples).
- No `high` severity on any sample outside family 14 (high is reserved for chains and container anomalies).
- The obfuscated ProgID in 14-02 (`"WScr" & "ipt.Sh" & "ell"`) is resolved to `WScript.Shell` through constant folding.

## A6. CLI

- `macro-port scan corpus/ -o out --format json,html,csv` completes with exit code 0 (or 1 if malformed fixtures are included) and writes `report.json`, `report.html`, `files.csv`, `procedures.csv`.
- `--redact` output follows the redaction table of SPEC section 8: no `snippet` field, no `errors[].message`, no clear-text path or root (test: none of the corpus file paths, and none of the string literals of the corpus, appear in the output); with `--redact-identifiers`, no module or procedure name appears either. The same `--redact-key` gives identical output across runs; no key gives different hashes across runs.
- `macro-port rules check` rejects a rules file with an unknown key, an invalid regex or a duplicate signal id.
- Performance: 1,000 files (corpus replicated with varied names, including the 3,600-line 10-06 sample) analysed in under 60 seconds on a 4-core laptop with `--jobs 4`; peak memory under 1 GB.
- Binaries built in CI for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`.

## A7. HTML report

- Single file, opens from `file://` with no network request (test with a headless browser and network disabled; zero requests besides the file itself).
- Shows fleet totals, distribution by family, complexity and severity, and per-file drill-down with evidence lines.
- Injection tests: with the injection fixture (SPEC section 8), the HTML report executes no script (headless browser, no dialog, no console error from injected code) and every CSV cell starting with `=`, `+`, `-` or `@` is prefixed with `'`.
- Fully available in English and French (`--lang fr`): no missing Fluent key in either language (test).

## A8. Privacy and safety

- No network call at any time: tests run the CLI in a network-denied sandbox; the binary has no HTTP client dependency (`cargo deny` ban list).
- No code from analysed files is executed or evaluated (code review checklist, *manual*), and no temporary files are written outside the output directory.
- `cargo deny check` passes: licences compatible with Apache-2.0, no known vulnerable crate.

## A9. WASM and mini web app

- For each corpus file, `analyze()` returns a report whose `files[0]` is identical to the corresponding entry of the CLI's `report.json` produced with `--deterministic`, after normalising `path`; `tool.build` and `generated_at` are excluded.
- A 5 MB `.xlsm` is analysed in under 3 seconds in Chromium and Firefox on a standard laptop, without freezing the UI (analysis in a Web Worker).
- The app loads and works offline after first load; its Content-Security-Policy forbids network connections (`connect-src 'none'`); an end-to-end test (Playwright) checks that analysing files triggers no network request.
- Multi-file and folder drop, fleet view, file view with evidence, filters, JSON and CSV export, EN/FR switch: covered by end-to-end tests on a sample of 20 corpus files.
- *Manual*: usability review by the owner on a real fleet of at least 50 files.
