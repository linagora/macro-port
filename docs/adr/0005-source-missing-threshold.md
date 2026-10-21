# ADR-0005: Threshold of `source_missing_suspected`

Date: 2026-10-21 · Status: proposed (to validate) · Decided by: night agent for ticket #36, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M1 step 8 suspects a module of having lost its source (VBA stomping: the source is removed, the compiled code stays) when its stream is present, its decompressed source is empty or holds only `Attribute` lines, and its compiled part, the bytes before `TextOffset`, is "non-trivial (more than 512 bytes)". The extraction then raises `sec.source-missing` (high).

CORPUS-PLAN row 9 keeps real stomped files out of the repository and allows a crafted case in a Rust test. The fixtures hold none.

## Measures

A throwaway probe (2026-10-21) read every module of the `.xlsm`, `.xlsb` and `.xls` fixtures of `corpus/fixtures/generated/` and of the `.xlsx` of `corpus/fixtures/malformed/`: 684 modules.

- 451 modules have a source of blank lines and `Attribute` lines only: the default document modules (`ThisWorkbook`, `Sheet1` and the like). Their `TextOffset` is 819 bytes for 433 of them and 827 for 18. The three binary formats agree: `.xls` 145 and 6, `.xlsb` 143 and 6, `.xlsm` 143 and 6, and the two of the `.xlsx` at 819. The 827 are two modules of each of three workbooks (`06-01`, `11-03`, `11-05`) in each format; I do not know why they are 8 bytes larger.
- 233 modules hold code. Their `TextOffset` runs from 767 to 160,511 bytes. Seven are at or below 1,024 (767 three times, 815, 991 three times) and twelve more are below 1,200.

With 512, all 451 modules of the first group would be suspected: every workbook that Excel writes would raise a high-severity signal. No module of the corpus has an empty source and a compiled part other than 819 or 827, so no fixture is a positive case.

The corpus holds no empty module of another type, so what an empty standard module compiles to is not measured. An extrapolation, which is not a measure: the 77 code modules of the `.xlsm` fixtures fit `TextOffset = 595 + 2.59 x (length of the compressed source)` (least squares, R2 0.995), which puts a module with no source near 595 bytes. The fit takes the code modules whatever their type, and a form or a class module may sit elsewhere.

## Decisions

The compiled part is non-trivial above 1,024 bytes. It is a constant, `TRIVIAL_COMPILED_PART` in `analyze/project.rs`, 197 bytes above the largest compiled part of an empty module in the corpus. The other conditions are those of the SPEC: the source is empty or holds blank lines and `Attribute` lines only (a line whose first word is `Attribute`, in any case), and a module whose source could not be read is neither suspected nor cleared (`source_missing_suspected` is null).

The positive cases are crafted in tests, as CORPUS-PLAN row 9 allows. `tests/analyze/failures.rs` empties the source container of the code module `M_01_01` of a valid project, in an `.xlsm` and in an `.xls`, and keeps its 3,030 bytes of compiled part: the module is suspected, the other two are not, the file has the signal `sec.source-missing`. The boundary (1,024 bytes not suspected, 1,025 suspected) is tested in `analyze/project.rs`, and the A1 test requires `false` for every module of every fixture.

A file whose only sign of macros is a module suspected of having lost its source has the status `ok`. The table of SPEC M1 gives `ok` to a file with a procedure, and the procedures of such a module are compiled but out of sight: `no_macros` would call harmless the file that the signal exists to catch.

## Alternatives rejected

- **512 bytes, as the SPEC says.** It flags every default document module, as measured above.
- **A threshold for each module type.** The empty modules of the corpus are all documents, so the compiled part of an empty standard module, class module or form is not measured. The extrapolation above would allow a threshold near 700 for standard modules, which would catch the module of `14-01` (991 bytes, see the consequences), but it rests on a fit and not on an empty module, and a form with many controls and no code could compile to more. A wrong guess costs a high-severity signal on every file with such a module, where the single threshold costs a miss on small modules. Revisit it with real files.
- **The threshold in `rules/security.yaml`.** The SPEC puts the security logic in the rules, but `container_indicators` carry an id, a severity and a label, and a new key needs a change of the rules schema and of the typed rules, which is a contract change. The constant is one line to move there later.

## Consequences

- A stomped module whose compiled part is 1,024 bytes or less is not suspected. The code modules of the corpus show how small that is: a few lines of code compile to 767 to 991 bytes, so a very small module can be stomped without being flagged. The module of `14-01-auto-open-shell` has a compiled part of 991 bytes: with its source emptied (a throwaway probe on the `.xls`, 2026-10-21), the file raises no signal, the module has `source_missing_suspected` false and the status is `no_macros`. The signal is the weakest where the code is the shortest.
- A module that is empty, is not a document, and compiles to more than 1,024 bytes would be a false positive. No fixture has an empty standard module, class module or form, so I cannot say whether one exists; a form with many controls and no code is the case I would check first (a hypothesis, no sample).
- The positive cases are crafted from valid files, not written by an attacker's tool. Real stomped samples, handled in isolation as CORPUS-PLAN row 9 describes, would test the threshold on compiled parts that nobody shaped for the test.
- The measures come from one Excel, version 16.0 (`excel_version` in `fixtures.yaml`). Another version may write a larger compiled part for an empty module, and every workbook it saves would then raise a high-severity signal. Real files from many Excel versions (CORPUS-PLAN row 6) would show it; until then the margin of 197 bytes is the only protection.
- When the fixtures are rebuilt with another Excel and the size of the compiled part of an empty document module grows by more than 197 bytes, the A1 test fails.
