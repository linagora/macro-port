# ADR-0007: Redaction of a report for sharing

Date: 2026-10-21 · Status: proposed (to validate) · Decided by: night agent for ticket #52, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC section 8 gives the table of `--redact`: a keyed HMAC-SHA256, cut to 16 hexadecimal characters, replaces the paths, the roots and the name of the VBA project; the snippets are removed; the messages of the errors read `[redacted]`; the references stay; the names of the modules and procedures stay unless `--redact-identifiers` hashes them. ACCEPTANCE A6 checks the result: no snippet, no clear path or root, the same key gives the same output, no key gives other hashes in each run, and with `--redact-identifiers` no module or procedure name.

The SPEC leaves open:

- who draws the random key, since `macroport-core` is pure and may not draw random bytes;
- what "module and procedure names" covers: the report writes the names of the project in more places than the list of modules and procedures (the stream of a module, the procedures that a procedure calls, the module and the procedure of a signal);
- how "the original extension" survives when the text after the last dot can itself be a name that the redaction hides;
- whether the order of the files, which follows their paths, changes;
- how the JSON, the HTML and the CSV stay the same report;
- what `--no-snippets` does alone, and which options go with `--redact`.

## Decisions

**One rewrite of the model.** `macroport_core::redact::Redaction` rewrites the `Report` in place. The CLI applies it once, after the scan and before any report is rendered, so that the JSON, the HTML and the CSV carry the same values and no renderer knows about the key or the table. `redact::remove_snippets` is the part that needs no key, and what `--no-snippets` runs.

**Hash.** The first 8 bytes of the HMAC-SHA256 of the UTF-8 text under the key, written as 16 lowercase hexadecimal digits. The same text gives the same hash wherever it appears, which keeps the links of the report: the module that a signal names is the module of the list. With 64 bits, a collision takes billions of distinct names to be likely, and it would merge two names in a report, not reveal one.

**Key.** The core draws nothing: `Redaction::new(key, identifiers)` takes the bytes of the key, of any length (the `hmac` crate types a failure that no key raises, and the core hands it on as `KeyError` instead of panicking). In the CLI, `--redact-key KEY` gives the bytes of the argument. Without it, the CLI draws 32 random bytes with `getrandom` (ADR-0002; the only use of randomness in the program), once per run and before the scan, and neither prints nor writes them: two runs cannot be linked. A system that gives no random bytes is known before minutes of analysis: exit code 3 with `cli-error-key`. An empty `--redact-key` is a usage error (exit code 2): the HMAC under an empty key is a function that anyone can compute. The hashes are as strong as the key: with a short key, whoever holds a report can test guesses of paths and names. `--deterministic` does not fix the random key (it fixes the date and the duration, SPEC M4): a test that needs the same bytes passes `--redact-key`.

**Paths and roots.** The hash of the whole path as the report gives it, then a dot and the extension of the last name if, ignoring case, it is one of the twelve that a scan reads (SPEC M4), written as it was. Any other text after the last dot could be part of a name that the redaction hides (`invoice.acme-corp`). The whole path is hashed in one piece, so the report does not show the depth of the tree or which files share a directory.

**Project name.** Hashed as written. Nothing else in the report names the project, so there is nothing for the hash to stay consistent with.

**Identifiers** (`--redact-identifiers`). Every name that the project defines and the report holds: `name` and `stream_name` of a module, `name` of a procedure, the entries of `calls`, and `module` and `procedure` of the signals of a file, a module and a procedure. VBA ignores case, so a name is lowercased before it is hashed: `Module1` in a definition and `MODULE1` in a call give one hash. Modules and procedures share one namespace. An entry of `calls` is cut at the dots and each part is hashed by itself, so that `Module1.Run` stays linked to the module and the procedure it names (the schema says that `calls` lists procedures of the same project, so every part is a name of the project).

**What stays.** The references, `apis[].name` (object model members, functions and ProgIDs, normalised), the metrics, the families, the signals (ids, severities, line numbers), the traits, the statuses, the ids of rules, the codes of errors, the `summary`, and the hashes of contents `files[].sha256` and `modules[].source_sha256`. The report holds no name of a declaration of the project: `declarations` counts them.

**Order of the files.** The files are sorted again by hashed path, then by hash of content, as a report always is. Keeping the order of the clear paths would show it.

**The `redacted` flag.** `true` only with `--redact`, which hashes the paths. `--no-snippets` alone removes the snippets and leaves `redacted` false: its report still carries paths and names, and must not be taken for one that can be shared.

**Options.** `--redact-key` and `--redact-identifiers` require `--redact` (usage error, exit code 2): taken silently, `--redact-identifiers` alone would give a report with every name in it to someone who asked for them to be hidden. `--redact` includes `--no-snippets`. `file` has no privacy option: the SPEC gives them to `scan`, and `file` prints one report on the console of the person who runs it.

**Console.** Not redacted, since it shows nothing to hide: the progress and the summary give counts and the output directory typed by the user.

**A guard.** A test of the core walks `schema/report.schema.json` for every property that holds free text and compares them with a list that says what the redaction does with each (`FREE_TEXT` in `tests/redact.rs`). A field that the schema gains fails the test until someone decides it.

## Alternatives rejected

- **Redaction in each renderer.** Three implementations of one table, and a renderer that misses a field leaks in one format only.
- **Hashing the components of a path one by one.** It shows the depth of the tree and which files are siblings, and gives the same hash to the same directory name everywhere.
- **Keeping any extension.** The text after the last dot can be a name.
- **A namespace of hashes for each kind of name.** The entries of `calls` do not say whether a part is a module or a procedure. With one namespace, a module and a procedure that share a name get one hash, which links nothing that was not linked.
- **Folding the case of the project name.** It is not an identifier of the code, and nothing refers to it.
- **Printing the random key at the end of the run**, to let the person reproduce the hashes. It writes the means of linking the reports in a log. Whoever wants to link runs passes a key.
- **Setting `redacted` for `--no-snippets`.** A reader would take the paths for hashed.
- **A key from a file or an environment variable.** The SPEC fixes `--redact-key KEY`. A file or a variable would keep the key out of the list of processes; it can be added without changing the report. This is a scope choice, not a judgement on merit.

## Consequences

Points for the owner:

- **The hashes of contents stay**, `files[].sha256` (required by the schema) and `modules[].source_sha256`. Whoever holds a candidate file can check that a redacted report is about it, and two reports on the same file can be linked although each was made under a random key. SPEC section 8 says that a random key makes runs "unlinkable": this is not true of the content hashes. Keying them breaks their use as the identity of a file (looking up a sample in a database, for instance); dropping `sha256` changes the schema. The choice is yours.
- **`apis[].name` stays**, and the SPEC's table does not list it. It holds ProgIDs (`CreateObject:Vendor.Component`), so a component of the organisation, or a library it references, can name the organisation. ACCEPTANCE A6 also says that "none of the string literals of the corpus" appears in a redacted output, which a ProgID kept in `apis[].name` contradicts to the letter. The check at the level of literals is not written (it needs the analysis of M2, and the ambiguity decides what it asserts). Options: keep ProgIDs as the vocabulary of the report, hash those that no rule knows, or read A6 as "except the vocabulary of the report".
- **The references stay**, as the SPEC says. A reference to an internal library can identify the organisation (a hypothesis: I have not seen the display names of the references of a real project).

What follows from the decisions:

- A key on the command line shows in the list of processes and in the history of the shell.
- Until the analysis finds modules and procedures (M1, M2), the CLI tests cannot check on the corpus that the names of a project are hashed; that test is written and ignored, and the redaction itself is tested on the hand-written reports (`crates/macroport-core/tests/redact.rs`, `crates/macroport-cli/tests/scan_redact.rs`).
- The core stays free of randomness for M5 too: `macroport-wasm` is to draw the key (ADR-0002 gives it `getrandom` with `wasm_js`) and hand it to `Redaction::new`. Its `analyze` redacts one file at a time, so a web app that aggregates the files of a fleet has to pass one `redactKey` for all of them, or the same name gets another hash in each file.
