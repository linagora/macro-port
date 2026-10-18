# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Macro-Port is an open source (Apache-2.0) analyzer of Excel VBA macros that helps organisations migrate from Microsoft Excel to OnlyOffice Spreadsheet, Grist and LibreOffice Calc. It is initiated by LINAGORA. Read these files before any work:

1. `docs/SPEC-LOT1.md`: what to build now, milestone by milestone.
2. `docs/ACCEPTANCE.md`: how the work is judged.
3. `docs/adr/`: decisions already taken (do not reopen them silently).
4. `schema/report.schema.json` and `rules/*.yaml`: the contracts.

## Current scope

**Lot 1 only** (inventory): extraction, metrics, security signals, family classification, JSON/HTML/CSV reports, CLI and WASM mini web app. Work milestone by milestone (M0 to M5, M0 being the workspace, CI and rules-loading PR); do not start a milestone before the previous one is merged. Exception: until binary fixtures exist in `corpus/fixtures/generated/`, do M2 and M3 first on the text corpus via `analyze_modules`. M5 always comes last.

Never, in Lot 1: call an LLM, translate or convert VBA code, score compatibility with target suites, fill the `orientation` field, add network access or telemetry.

## Current state of the repository

Still the starter kit: no Cargo workspace yet, and `crates/`, `schema/rules/` (M0), `i18n/` (M4) and `web/` (M5) do not exist. Until M0 is merged, only the Python tools run. Until M4, CLI messages are English only; Fluent (`en`, `fr`) arrives with the rest of the CLI strings. The binary fixtures (`corpus/fixtures/generated/`, `malformed/`) were built on 2026-10-18 on branch `fixtures/initial`; once it is merged, the M2/M3-first exception no longer applies and M1 follows M0. Update this section in the PR that changes it.

## Non-negotiable engineering rules

- **Never execute or evaluate** anything from an analysed file. The only "evaluation" allowed is the closed constant-folding set of SPEC M2 item 4 (literal concatenation, `Chr`/`ChrW`/`ChrB` of numeric literals, `StrReverse`/`LCase`/`UCase` of foldable values, module-level `Const` strings).
- **Every string from an analysed file is untrusted output**: escape it in HTML (no `innerHTML`), protect CSV cells against formula injection, follow the redaction table (SPEC section 8).
- `macroport-core` is pure: bytes in, `Report` out. No `std::fs`, no `std::net`, no `std::env`, no clock, no randomness, no threads. `#![forbid(unsafe_code)]`.
- Treat every input as hostile: no `unwrap()`, `expect()`, indexing or arithmetic that can panic on input-dependent data. Use checked arithmetic and bounded reads. Respect the limits in SPEC M1.
- Deterministic output: stable ordering (`BTreeMap`, sorted vectors), no hash-map iteration order in output.
- Family, security and complexity logic lives in `rules/*.yaml`. Rust only implements the generic matchers and the closed list of structural predicates in SPEC section 7. If a rule cannot be expressed, propose a new matcher in an ADR instead of hard-coding a family.
- Every source file starts with `// SPDX-License-Identifier: Apache-2.0`.
- New dependency: check licence compatibility (`cargo deny`), prefer well-maintained crates, justify any heavy one in the PR description. No HTTP client crate anywhere in the dependency tree, dev-dependencies included (watch default features: `jsonschema` needs `default-features = false`, `zip` only `deflate`).

## Repository layout

```
crates/macroport-core   # pure analysis library
crates/macroport-cli    # `macro-port` binary
crates/macroport-wasm   # wasm-bindgen bindings
web/                    # mini web app (TypeScript, Vite, Preact)
rules/                  # families.yaml, security.yaml, complexity.yaml (CC BY 4.0)
schema/                 # report.schema.json (public contract)
i18n/en, i18n/fr        # Fluent .ftl files
corpus/                 # test corpus: families/ (text samples), fixtures/ (binary), ground-truth/
docs/                   # SPEC, ACCEPTANCE, ADRs, CORPUS-PLAN
tools/rules-prototype/  # Python calibration prototype: reference only, never port it
tools/fixtures/         # Build-Fixtures.ps1 (Windows + Excel only, never in CI), make_malformed.py
LISEZMOI-KIT.md, PROMPT-*.md  # owner-facing kit notes and prompts, in French
```

## How the pieces fit

- `macroport-core` pipeline: container detection (OOXML zip, CFB/BIFF8) → MS-OVBA extraction (`dir`, `PROJECT` and module streams) → lexer → structural parser (logical lines, `With` resolution, constant folding, call graph) → metrics → rule engine → `Report`. Two entry points: `analyze(bytes, file_name)` and `analyze_modules(modules, file_name)`. The second skips the container layer (`container: "text_module"`): that is how M2 and M3 run on `corpus/families/` before binary fixtures exist.
- Everything impure lives in the CLI: directory walking, `rayon`, output files, HTML and CSV rendering, Fluent, and the injected `generated_at` and `scan.duration_ms` (`--deterministic` fixes them for tests). `macroport-wasm` wraps `analyze` for one file; the web app only renders report JSON and aggregates single-file reports into a fleet report.
- Evidence line numbers count every physical line as the file presents it, export headers, `Attribute` lines and the corpus header block included (SPEC section 5). Never renumber after skipping them.
- Rules are embedded at build time and overridable with `--rules`. `corpus_slug` in `families.yaml` maps a family id (`01-formatting`) to the corpus folder and manifest `family` (`01-mise-en-forme`); A4 relies on it. Family 14 has no family rules: it is only scored against `corpus/ground-truth/security-expected.yaml` (A5).
- `container_indicators` in `security.yaml` are raised by the extraction layer (M1), not by matchers; the YAML only holds their labels and severities.
- A1 compares extracted source with Excel's own export of each module (`*.expected.bas|cls`, listed in `corpus/fixtures/fixtures.yaml`), not with the corpus text.

## Commands

Available now. `-I` ignores user site-packages: install PyYAML in a venv and run that venv's `python -I`.

```
python3 -I corpus/tools/build_manifest.py   # rewrites corpus/manifest.json and .csv from corpus/families/*/manifest.json; exits 1 on any inconsistency
python3 -I tools/rules-prototype/prototype.py . [--verbose] [--report out.json]   # baseline A4/A5 metrics of the rules (needs PyYAML)
python3 -I tools/fixtures/make_malformed.py corpus/fixtures/generated corpus/fixtures/malformed   # needs the generated fixtures
```

Prototype baseline on rules v0.1.0: 74 samples, top-1 70 %, top-2 88 %, 17/17 expected security signals.

From M0 on:

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test -p macroport-core <name_filter>   # a single test or group
cargo insta review                           # review snapshot diffs (cargo-insta)
cargo deny check
cargo run -p macroport-cli -- scan corpus/families -o target/out --format json,html,csv
cargo run -p macroport-cli -- rules check
wasm-pack build crates/macroport-wasm --target web
(cd web && npm ci && npm run build && npm test)
```

## Tests

- Snapshot tests (`insta`) for parser output on every corpus sample; review snapshot diffs, never accept them blindly.
- Corpus tests compute A4 and A5 metrics and fail below thresholds; print the confusion matrix.
- Every JSON produced in tests is validated against `schema/report.schema.json`.
- Malformed fixtures and fuzz targets for every binary parser.
- Do not edit the corpus manifests (`corpus/families/*/manifest.json` and the root `corpus/manifest.json` and `.csv` generated from them), `corpus/ground-truth/` or corpus samples to make a test pass. If you believe the ground truth is wrong, say so in the PR and let the owner decide.

## Changing contracts

- `schema/report.schema.json`: bump `schema_version` (minor for additive, major for breaking), update the example, write an ADR.
- `rules/*.yaml`: bump the file `version`, report before and after A4/A5 metrics in the PR, no sample-specific patterns.

## Pull requests (the owner reviews alone: make reviews cheap)

- One milestone may span several PRs; each PR is small and thematic (target under 600 changed lines, excluding snapshots and fixtures).
- CI must be green before asking for review.
- Start every PR description with a **French summary** for the reviewer:
  - « Ce qui change » (3 to 6 lines),
  - « Décisions à valider » (choices you made that the spec did not settle, with the alternative you rejected),
  - « Écarts à la spec » (if any) and « Métriques » (A4/A5 numbers when rules or parser change).
- Then the technical description in English.
- Code, comments, commit messages, CLI and developer docs in English. User-facing strings through Fluent in English and French.

## When in doubt

Stop and ask in the PR or issue rather than guessing on: scope (is it Lot 1?), contracts (schema, rules format), anything touching safety (execution, network, file writes), licences.
