# ADR-0002: Lot 1 dependencies

Date: 2026-10-18 · Status: accepted · Decided by: Michel-Marie Maudet (LINAGORA)

## Context

SPEC section 4 gives dependency guidance and leaves the final choice to this ADR. The constraints come from the SPEC and `CLAUDE.md`:

- `macroport-core` is pure (no I/O, clock, environment, randomness or threads), forbids `unsafe` and compiles for `wasm32-unknown-unknown`, so no C code may sit under it.
- Every analysed file is hostile. On parsing paths, prefer crates that forbid `unsafe` themselves and let the caller bound every read.
- No HTTP client anywhere in the dependency tree, dev-dependencies included (A8).
- Licences compatible with distributing Macro-Port under Apache-2.0.
- Deterministic output.

## Method

Checked on 2026-10-18, and again on 2026-10-19 after review, with rustc 1.97.1 and cargo-deny 0.19.6. The probe used two throwaway workspaces, run against the repository's own files. The first held a core crate, a CLI crate and a wasm-bindgen crate, with the versions and features below and `insta` as a dev-dependency of the core. The second was a fuzz workspace with `libfuzzer-sys` and `arbitrary`.

- `cargo build --target wasm32-unknown-unknown` succeeds for the core crate and for the wasm-bindgen crate, with each of the two zip configurations discussed below.
- `cargo tree` shows no C build dependency (`cc`, `cmake`, `bindgen`) under the core. The only `*-sys` crate there is `js-sys`, the JavaScript bindings of wasm-bindgen on wasm32, which has no build script. `cargo tree` also shows no HTTP client or TLS crate in either workspace, across every target and every kind of edge.
- `cargo deny check` passes for advisories, bans, licences and sources in both workspaces, with the policy of the last section. Two changes make `bans` fail:
  - adding `jsonschema` with its default features, even as a dev-dependency only, fails on `reqwest`, `hyper`, `hyper-util`, `h2` and `rustls`;
  - switching zip to its `deflate` feature fails on `zlib-rs` and `zopfli`.
- The three shipped rules files parse with serde-saphyr. The 45 patterns of the regex-based matchers (`regex`, `string`, `comment`, `procedure_name`, `reference`) all compile with `regex`, case-insensitive.
- `schema/examples/report.example.json` validates with jsonschema, with `format` asserted. The same report with `generated_at: "yesterday"` fails with `"yesterday" is not a "date-time"` at `/generated_at`.
- The other probe results, and what the crate sources show, are quoted in the entries they support.

Versions, licences and release dates come from crates.io on 2026-10-18, and from crates.io and npm on 2026-10-19 for cargo-fuzz and Playwright. "12 m" counts the stable releases of the twelve months to 2026-10-18, yanked ones excluded.

## Decisions

### Containers and extraction (core)

| Crate | Purpose | Features | Licence | Maintenance | Rejected |
| --- | --- | --- | --- | --- | --- |
| `cfb` 0.15.0 | reads CFB (OLE2) containers: `.xls` files and `vbaProject.bin` | none (the crate has none) | MIT | 2026-09-18, 4 in 12 m, since 2017 | `ole` 0.1.15: WTFPL (not on the allow-list), no release since 2018. An own MS-CFB reader: more code to fuzz, reconsidered only if fuzzing shows `cfb` unfit. |
| `zip` 9.0.5 | reads OOXML packages | `default-features = false, features = ["deflate-flate2"]` | MIT | 2026-10-17, 20 in 12 m | `features = ["deflate"]`, see below. `rc-zip` 5.4.1 (sans-I/O, 2025-11-19): fallback if M1 finds that zip's entry reader cannot support the SPEC M1 caps. `rawzip` 0.5.2: first release 2025-01. |
| `flate2` 1.1.10 | supplies zip's deflate backend | default (`rust_backend`, i.e. `miniz_oxide` 0.9.1) | MIT OR Apache-2.0 | 2026-08-28, 4 in 12 m | `zlib-rs` backend, see below. |
| `quick-xml` 0.42.0 | reads `[Content_Types].xml`, the relationship parts and `workbook.xml` | default (empty: no `encoding`, no `serialize`) | MIT | 2026-08-22, 10 in 12 m | `roxmltree` 0.21.1: a DOM, no release in 12 m. `xml-rs` 1.0.0: no advantage for these parts. |
| `codepage` 0.1.3 | maps the VBA project's code page to an encoding | default | Apache-2.0 OR MIT | 2026-09-12, 1 in 12 m | `oem_cp` 2.1.2: DOS/OEM tables only. Hand-written tables. |
| `encoding_rs` 0.8.42 | decodes text in that encoding | default | (Apache-2.0 OR MIT) AND BSD-3-Clause | 2026-09-24, 3 in 12 m | none: `codepage` maps to it. |

- **cfb**: no `unsafe` in its sources. At open it rejects a FAT where a sector is pointed to twice and a chain that returns to its first sector, so FAT loops surface as errors; it also rejects loops in the directory tree. The entry count and storage depth limits of SPEC M1 remain our checks. It depends on `web-time`, which pulls `js-sys` and `wasm-bindgen` into the core's graph on wasm32. It calls `SystemTime::now` only on write paths (creation, `touch`, `set_modified_time`). The core never uses them, and M1 adds them to the core's `disallowed-methods` along with cfb.
- **zip** departs from SPEC section 4's guidance, which names `features = ["deflate"]`. In zip 9 that feature enables two backends:
  - `deflate-zopfli`, a compressor, which is dead code for a reader;
  - `deflate-flate2-zlib-rs`, that is `zlib-rs` 0.6.8, which has 456 lines containing `unsafe` in its sources.

  `deflate-flate2` turns on zip's flate2 support without choosing a backend: zip depends on flate2 with `default-features = false`, and flate2 refuses to compile without one (`compile_error!` in its `lib.rs`). The core's direct `flate2` dependency supplies the backend through its default `rust_backend`, so the inflate loop is `miniz_oxide`, which declares `#![forbid(unsafe_code)]`. The checksums keep some `unsafe` in both configurations: `crc32fast` (15 lines, used by zip and flate2) and `simd-adler32` (36 lines, which flate2 turns on in miniz_oxide). Both configurations are pure Rust and both build for wasm32, so either keeps the SPEC's "no C codec". This ADR prefers the safe-Rust inflate loop for attacker-controlled data. The speed difference has not been measured. The hypothesis is that miniz_oxide inflates more slowly than zlib-rs; the A6 performance gate will show whether that matters. `deny.toml` bans `zlib-rs` and `zopfli`, so turning on `deflate` fails `cargo deny`.
- **quick-xml** declares `#![forbid(unsafe_code)]`. It is a pull parser. Without its `encoding` feature it reads UTF-8 only, which is what Excel writes; M1 decides the status of a part in another encoding.
- **Code pages**: the probe decoded samples in 1252 (windows-1252), 1251 (windows-1251), 932 (Shift_JIS), 1200 (UTF-16LE), 10000 (macintosh) and 65001 (UTF-8) correctly. `codepage::to_encoding` returned `None` for 0 and 12345. Its source maps 50225, 50227 and 52936 to `Some(REPLACEMENT)`, the WHATWG encoding that decodes everything to U+FFFD. The core calls `to_encoding_no_replacement`, which returns `None` for those three too, so they fall into the unknown case. The core handles 1200 and unknown values explicitly, as SPEC section 4 asks; M1 decides what each case yields.
- **MS-OVBA and BIFF8** (the `dir`, `PROJECT` and module streams, decompression, FILEPASS, and BOUNDSHEET for XLM sheets): our own code, no crate. Rejected:
  - `ovba` 0.7.1: its licence is not in SPDX form, and it has had no release since 2024-12-22;
  - `vba_extract` 0.2.0: it only copies `vbaProject.bin` out of the zip (2024-08-21);
  - `calamine` 0.36.1: MIT and active, but it reads VBA modules only as a side feature of a cell-data reader. Lot 1 needs every `dir` and `PROJECT` record, the references and the protection flags, under the SPEC M1 limits on each read.

### Rules, report and redaction (core)

| Crate | Purpose | Features | Licence | Maintenance | Rejected |
| --- | --- | --- | --- | --- | --- |
| `serde` 1.0.229 | derives the rules and report types | `derive` | MIT OR Apache-2.0 | 2026-07-18, 1 in 12 m | none |
| `serde_json` 1.0.154 | writes the JSON report; gives the CLI rules documents to validate | default (no `preserve_order`, no `arbitrary_precision`) | MIT OR Apache-2.0 | 2026-10-11, 9 in 12 m | a hand-written JSON writer |
| `serde-saphyr` 2.0.0 | parses the YAML rules files | `default-features = false, features = ["deserialize"]` | MIT OR Apache-2.0 | 2026-10-13, 28 in 12 m, first release 2025-09-27 | see below |
| `regex` 1.13.1 | compiles the regexes of the rules | default | MIT OR Apache-2.0 | 2026-07-15, 4 in 12 m | `fancy-regex` 0.19.2, `pcre2`, see below |
| `sha2` 0.11.1 | `files[].sha256` and `source_sha256` | default | MIT OR Apache-2.0 | 2026-10-12, 2 in 12 m | `ring`, `aws-lc-rs`: C and assembly |
| `hmac` 0.13.0 | the HMAC-SHA256 redaction of SPEC section 8 | default | MIT OR Apache-2.0 | 2026-03-29, 1 in 12 m | same |
| `thiserror` 2.0.21 | error types | default | MIT OR Apache-2.0 | 2026-09-23, 4 in 12 m | `anyhow` in the CLI: type-erased errors make the exit codes 1, 2 and 3 of SPEC M4 harder to keep exact. Hand-written `Display` impls. |

- **serde_json** without `preserve_order` keeps `serde_json::Map` a `BTreeMap`, so object keys come out sorted.
- **serde-saphyr** is the YAML 1.2 parser that SPEC section 4 asks this ADR to choose.
  - Features left off: `serialize`, `comments`, `include`, `include_fs`, `properties`.
  - The rules loader sets `strict_booleans: true`, `merge_keys: MergeKeyPolicy::Error` and `reject_unsupported_tags: true`. It keeps the defaults `duplicate_keys: Error` and the parse `budget`.
  - The probe showed:
    - typed `String` fields keep `yes` and `no` as strings. Read into an untyped value with the default options, they become booleans (YAML 1.1); with `strict_booleans`, they stay strings (YAML 1.2 core schema);
    - a duplicate key and an unknown field (`#[serde(deny_unknown_fields)]`) are rejected with line, column and a source snippet;
    - a `<<` merge key and an `!include` tag are rejected;
    - an alias bomb (eleven levels of tenfold aliases) stops with `budget breached: Nodes { nodes: 250001 }`.
  - It declares `#![forbid(unsafe_code)]`.
  - Rejected: `serde_yaml` (archived, `0.9.34+deprecated`); `serde_norway` 0.9.42 (a fork of serde_yaml, no release since 2024-12-21); `serde_yml` 0.0.13 (0.0.x); `saphyr` 0.2.2 without serde (every rules type mapped by hand).
- **regex** compiles to finite automata only: the crate guarantees search time linear in the size of the pattern and of the searched text, so a hostile VBA module cannot trigger catastrophic backtracking. It supports no lookaround and no backreferences. The shipped rules use neither (45 of 45 patterns compiled, inline `(?i)` flags accepted), and a `--rules` pattern that needs them fails `rules check` instead of pulling in a backtracking engine. Compilation stays under the builder's default limits (10 MiB for the compiled program, 2 MiB for the lazy DFA cache), so a pathological rule fails to compile instead of exhausting memory. Rejected: `fancy-regex` (backtracking for lookaround and backreferences, super-linear on hostile input); `pcre2` (C).
- **sha2 and hmac** (RustCrypto, pure Rust) compute the hashes and the redaction in the core. The caller always injects the key.

### CLI

| Crate | Purpose | Features | Licence | Maintenance | Rejected |
| --- | --- | --- | --- | --- | --- |
| `clap` 4.6.7 | parses the command line; help and usage errors | default + `derive` | MIT OR Apache-2.0 | 2026-09-14, 20 in 12 m | `lexopt`, `pico-args`, `bpaf`: lighter, with hand-written help and errors |
| `walkdir` 2.5.0 | walks the input directories | none | Unlicense OR MIT | 2024-03-01, none in 12 m | `ignore` 0.4.33: honours `.gitignore` and hidden-file rules, wrong for an inventory. `std::fs::read_dir` recursion. |
| `rayon` 1.12.0 | analyses files in parallel | default | MIT OR Apache-2.0 | 2026-04-14, 1 in 12 m | a hand-made `std::thread::scope` pool |
| `humantime` 2.4.0 | formats `generated_at` | default | MIT OR Apache-2.0 | 2026-07-02, 1 in 12 m | `jiff` 0.2.38, `time` 0.3.55, `chrono`: calendar and time-zone machinery for one call. Hand-written date arithmetic. |
| `getrandom` 0.4.3 | draws the redaction key | default | MIT OR Apache-2.0 | 2026-06-17, 5 in 12 m | `rand`: a generator framework for 32 bytes |
| `fluent-bundle` 0.16.0 (M4) | renders the user-facing strings in English and French | default | Apache-2.0 OR MIT | 2025-05-22, none in 12 m | `fluent-templates`, `i18n-embed`: loaders and macros on top of fluent-bundle that we do not need. `rust-i18n`: not Fluent. |
| `unic-langid` 0.9.6 (M4) | language identifiers for fluent-bundle | default | MIT OR Apache-2.0 | 2025-05-09, none in 12 m | none: fluent-bundle's language identifiers |
| `askama` 0.16.1 (M4) | renders the HTML report | `default-features = false` + `derive`, `std` | MIT OR Apache-2.0 | 2026-09-04, 9 in 12 m | see below |
| `csv` 1.4.0 (M4) | writes the CSV reports | none | Unlicense OR MIT | 2025-10-17, none in 12 m | a hand-written writer |
| `base64` 0.23.1 (M4) | encodes the CSP hashes of the HTML report | default | MIT OR Apache-2.0 | 2026-08-04, 2 in 12 m | `'unsafe-inline'` in the report's CSP: an injected script would run |
| `jsonschema` 0.59.0 | validates the rules files and, in tests, the reports | `default-features = false` | MIT | 2026-10-16, 68 in 12 m | see below |

- **clap** exits with code 2 on a usage error (`USAGE_CODE` in `clap_builder`), the code SPEC M4 assigns.
- **walkdir** is small and stable (BurntSushi); it does not follow symlinks by default, as SPEC M4 requires.
- **rayon** runs on a pool sized by `--jobs`; results are sorted before output, so `--jobs` never changes the report (A3).
- **humantime** formats `generated_at` with `format_rfc3339_seconds(SystemTime::now())`. Probe output: `2026-10-18T07:47:28Z`; the epoch gives `1970-01-01T00:00:00Z`, the `--deterministic` value.
- **getrandom** draws the redaction key when `--redact-key` is absent (SPEC section 8), never in the core. In M5, `macroport-wasm` enables its `wasm_js` feature (`Crypto.getRandomValues`) for the same case.
- **fluent-bundle** is Project Fluent's reference implementation. SPEC section 4 names it, and ADR-0001 requires Fluent files. Its slow release cadence is accepted. Terminal and CSV output call `set_use_isolating(false)`; otherwise placeables come wrapped in U+2068 and U+2069.
- **askama** renders the HTML report from compile-time templates. Every expression in an `.html` template is HTML-escaped unless its value is marked as safe.
  - The template applies no filter, `safe` included. The renderer marks three values as safe, with askama's `Safe` wrapper in a field of the template's struct, so that the trust is in the type and not in a template that a later edit could change.
  - One is the report data that SPEC section 8 embeds as JSON in a `<script type="application/json">` block. Our code serializes that data with serde_json, then escapes `<`, `>`, `&`, U+2028 and U+2029 as `\u` sequences. HTML escaping would be wrong there: it would leave entities that the HTML parser does not decode inside a script element.
  - The other two are the page's own stylesheet and the script that filters its list of files, files of this crate that `include_str!` reads at build time. They are not templates: `{% include %}` would parse them as askama syntax.
  - Nothing else is marked as safe, so no string from an analysed file is written unescaped outside that JSON block. Tests count the uses of `Safe`, and reject any filter and any `filter` block in the template.
  - askama's default features are `config`, `derive`, `std` and `urlencode`; the report uses `derive` and `std`. `config` reads `askama.toml` and pulls `basic-toml`, which RUSTSEC-2026-0374 reports as unmaintained, so `cargo deny` fails on it (`unmaintained = "all"`). The report needs neither a configuration file nor the `urlencode` filter.
  - askama's own `json` filter is not used. It needs the non-default `serde_json` feature, and it escapes only `&`, `'`, `<` and `>`, not U+2028 and U+2029.

  Probe: a path `"><script>alert(1)</script>` in an attribute and a snippet `<img src=x onerror=alert(1)> & 'q'` in text both came out as entities (`&#34;&#62;&#60;script&#62;…`).

  Rejected: `maud` 0.27.0 (no release since 2025-02-02); `minijinja` 3.0.1 (runtime templates, whose errors appear at run time instead of at build); hand-written HTML (one missed escape is an injection).
- **csv** does the RFC 4180 quoting. The UTF-8 BOM, and the `'` prefix of cells starting with `=`, `+`, `-`, `@`, tab or carriage return (SPEC section 8), are ours. A7 tests only the first four; prefixing all six also passes A7.
- **base64** encodes the SHA-256 hashes of the HTML report's inline style and inline script for its `Content-Security-Policy` (`'sha256-…'` sources), so that no other inline code can run. The CLI computes the hashes with sha2, at render time, over the text that the page carries.
- **jsonschema** serves `rules check` in the CLI, since SPEC section 7 validates rules against `schema/rules/*.schema.json`. As a dev-dependency, it also serves the report validation helper.
  - Features off: `resolve-http` (reqwest and rustls), `resolve-file`, `tls-aws-lc-rs` and `idna`. Format assertion still works (probe).
  - It is heavy: it brings 45 crates that the core does not already pull, among them `fancy-regex`, the `num` family and `parking_lot`.
  - Its `pattern` keyword only ever runs on our own schemas.
  - Rejected: `boon` 0.6.1 (no release since 2025-01-07); validating rules through serde types only, which would drop SPEC section 7's schema check from `rules check`.

### WASM (M5)

| Crate | Purpose | Features | Licence | Maintenance | Rejected |
| --- | --- | --- | --- | --- | --- |
| `wasm-bindgen` 0.2.129 | the JavaScript bindings of `macroport-wasm` | default | MIT OR Apache-2.0 | 2026-09-25, 20 in 12 m | hand-written `extern "C"` exports with manual memory passing |

Only `macroport-wasm` depends on it directly, and wasm-pack builds it.

### Tests and tooling

| Tool or crate | Purpose | Features | Licence | Maintenance | Rejected |
| --- | --- | --- | --- | --- | --- |
| `insta` 1.49.0 (dev) | parser and report snapshots, one per corpus file | `glob`, `yaml` | Apache-2.0 | 2026-10-03, 15 in 12 m | `expect-test`: no glob over the corpus. Hand-written golden files. |
| no crate | CLI end-to-end tests: `std::process::Command` on `env!("CARGO_BIN_EXE_macro-port")`, with outputs under `env!("CARGO_TARGET_TMPDIR")` | | | | `assert_cmd` 2.2.2 with `predicates`, and `tempfile` 3.27.0: conveniences only |
| cargo-fuzz 0.13.2 (tool) | builds and runs the fuzz targets of SPEC section 9 and A2, on nightly | | MIT OR Apache-2.0 | 2026-06-09, 1 in 12 m | `afl` 0.18.2: needs the AFL++ toolchain on CI. `bolero` 0.13.7: an abstraction over engines we do not need. |
| `libfuzzer-sys` 0.4.13 (fuzz) | the libFuzzer engine and `fuzz_target!` | default | (MIT OR Apache-2.0) AND NCSA | 2026-06-04, 3 in 12 m | none: the engine cargo-fuzz drives |
| `arbitrary` 1.5.0 (fuzz) | structured input, for a target that needs it | default | MIT OR Apache-2.0 | 2026-10-06, 1 in 12 m | decoding the raw fuzz bytes by hand |
| Playwright Test (`@playwright/test`, npm devDependency) | browser end-to-end tests on Chromium and Firefox: A7 in M4 (the HTML report opened from `file://`, with every request aborted and counted), A9 in M5 | | Apache-2.0 | 1.64.1 on 2026-10-14, 14 in 12 m; the version is pinned when first used | Rust WebDriver clients: `fantoccini` 0.22.1 pulls `hyper`, and `thirtyfour` 0.37.8 pulls `reqwest`, both banned. Puppeteer: no reason to depart from SPEC section 4, which names Playwright. |

`cargo-insta` and `wasm-pack` are tools installed by CI, not dependencies. The `fuzz/` workspace stays outside the main workspace, the wasm build and the stable CI, because libFuzzer is built from C++. The web app's npm packages (TypeScript, Vite and Preact, per the repository layout) are pinned in M5 with a committed lockfile.

## Cross-cutting rules

- Shared versions live in `[workspace.dependencies]`, and crates opt in with `workspace = true`.
- `Cargo.lock` is committed.
- One upgrade per commit. serde-saphyr and jsonschema, which release often, also get a read of their changelog with each upgrade.
- `default-features = false` wherever the defaults bring code we do not run: zip, serde-saphyr, jsonschema, askama.
- The highest MSRV among the retained crates is 1.89 (serde-saphyr): the toolchain the workspace pins (issue #2) must be at least that.
- Beyond `#![forbid(unsafe_code)]`, the workspace bans file system, network, environment, clock, randomness and thread APIs in the core, with clippy's `disallowed-methods` and `disallowed-types` (issue #2).
- A crate that this ADR does not list needs an amendment, or a new ADR, in the PR that adds it.

## cargo deny policy

The initial policy, which the workspace and CI ticket (#2) copies to `deny.toml` at the repository root. From then on, that file is authoritative: later exceptions and bans go there, not here.

```toml
# cargo-deny policy (ADR-0002). Applies to every target and to dev-dependencies.
[graph]
all-features = true

[advisories]
unmaintained = "all"
unsound = "all"
yanked = "deny"
ignore = []

[licenses]
allow = [
    "Apache-2.0",
    "BSD-3-Clause",
    "MIT",
    "MIT-0",
    "NCSA",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
]
unused-allowed-license = "allow"
exceptions = []

[licenses.private]
ignore = false

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true
deny = [
    { crate = "reqwest", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "hyper", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "hyper-util", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "h2", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "ureq", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "isahc", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "curl", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "curl-sys", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "attohttpc", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "minreq", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "surf", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "gloo-net", reason = "HTTP client: Macro-Port makes no network call (SPEC section 9, A8)" },
    { crate = "rustls", reason = "TLS stack: no Lot 1 crate needs one, its presence means a network client slipped in" },
    { crate = "native-tls", reason = "TLS stack: no Lot 1 crate needs one, its presence means a network client slipped in" },
    { crate = "openssl", reason = "TLS stack: no Lot 1 crate needs one, its presence means a network client slipped in" },
    { crate = "openssl-sys", reason = "TLS stack: no Lot 1 crate needs one, its presence means a network client slipped in" },
    { crate = "serde_yaml", reason = "archived; ADR-0002 chose serde-saphyr" },
    { crate = "zlib-rs", reason = "ADR-0002: zip inflates with miniz_oxide (deflate-flate2), not zlib-rs" },
    { crate = "zopfli", reason = "ADR-0002: a compressor, dead code for a reader" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

- **Scope**: there is no `targets` filter, so every target is checked. `all-features = true` turns on every feature of our own crates; third-party crates keep the features our manifests select, so the graph checked is the graph built. Dev-dependencies are included, as cargo-deny does by default; never set `exclude-dev`. Once the `fuzz/` workspace exists, it is checked with the same file.
- **Licences**: the allow-list covers the whole graph of both probe workspaces. The other licences found there are 0BSD, BSL-1.0, BSD-2-Clause, GPL-2.0-only, LGPL-2.1-or-later and Apache-2.0 WITH LLVM-exception. Each only appears as one branch of an `OR` whose other branch is allowed, which cargo-deny accepts. `unused-allowed-license = "allow"` because early PRs use only part of the list. Our own crates are checked too (`private.ignore = false`), and each declares `Apache-2.0`.
- **Advisories**: a known vulnerability always fails, and so do unmaintained or unsound crates and yanked versions. An exception goes in `ignore`, with a reason, in the PR that needs it.
- **Bans**:
  - HTTP clients (the SPEC list and close relatives), TLS stacks and `serde_yaml`;
  - `zlib-rs` and `zopfli`, which keep the zip deviation in force.

  Wildcard versions are denied, except for workspace path dependencies. Duplicate versions only warn.
- **Sources**: crates.io only.

## Consequences

- Later PRs add the crates listed here without reopening the choice.
- There is one deviation from SPEC section 4's guidance to validate: zip uses `deflate-flate2`, with the core's own `flate2` dependency as its backend, instead of `deflate`.
