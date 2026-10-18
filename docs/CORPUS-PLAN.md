# Macro-Port corpus completion plan

The v0.1 corpus (`corpus/families/`, 80 text samples in 14 usage families, plus 11 reference-only sources) is enough to develop the parser and the rules (M2, M3). It is not enough for extraction (M1): Lot 1 starts from binary files, and the corpus has none yet. This plan lists what to add, how, and who does it.

| # | What | Why (acceptance) | How | Who | Needed by |
| --- | --- | --- | --- | --- | --- |
| 1 | Binary fixtures generated from the text samples: each `.bas` / `.cls` saved as `.xlsm`, `.xlsb` and `.xls`, with Excel's own export of each module as reference | A1 extraction completeness | `tools/fixtures/Build-Fixtures.ps1` with Windows PowerShell 5.1 on one Windows workstation with Excel (about 30 minutes, once), output folder excluded from antivirus real-time scanning with the security officer's approval. Never in CI. | LINAGORA, one person with Excel | Start of M1 |
| 2 | Special containers: encrypted `.xlsm` and `.xls`, XLM macro sheet (`.xlsm`, `.xls`), workbook without macros | A1 statuses | Produced by the same script | Same | Start of M1 |
| 3 | Three password-protected VBA projects | A1 `vba_project_protected`, source still extracted | Manual protection of three generated `.xlsm` in the VBA editor, then update `fixtures.yaml` | Same | M1 |
| 4 | Code pages other than 1252 (1251 Cyrillic, 932 Japanese) | A1 decoding | Run the script once on a Windows VM with the system locale switched to Russian, then Japanese, using 2 samples whose strings contain non-ASCII text (add two small synthetic samples), or obtain such files from partners | LINAGORA | M1 |
| 5 | Malformed files | A2 robustness | `tools/fixtures/make_malformed.py` from items 1 and 2 (deterministic, runnable in CI); deep CFB nesting and broken compression chunks crafted in Rust unit tests | Claude Code | M1 |
| 6 | Real `.xlsm` files from the public web with permissive licences | Realism of containers produced by many Excel versions | Search GitHub repositories with a LICENSE file for committed `.xlsm`, `.xlsb`, `.xls` with VBA; also check the test data of permissively licensed analysis tools (e.g. oletools, BSD licence; verify which files are benign and redistributable before copying) | Claude Code, owner validates licences | M1 or M2 |
| 7 | Real anonymised workbooks from public partners (target: 50 to 200 files) | Usability review (A9 manual), calibration of complexity thresholds, rules beyond the textbook cases | Partnership with DINUM and one pilot ministry; anonymisation done by the partner (cell data replaced, code kept); files stay private (`corpus-private/`, never published) unless explicitly released | Owner | Before Lot 1 acceptance |
| 8 | Ground truth review | A4 and A5 measure against it | Owner (or a migration consultant) reviews `family`, `expected_target`, `migration_difficulty` for the 80 samples and signs off; disagreements recorded in the manifest `notes` | Owner | Before M3 |
| 9 | VBA stomping samples (source removed, p-code kept) | `source_missing` detection | Do not build them with offensive tooling. Either obtain benign examples from published security research datasets handled in an isolated environment, or craft a minimal case in Rust tests by blanking a module's source in a valid `vbaProject.bin`. Real malware never enters the repository. | Claude Code (Rust test) | M1 |

## Gaps already known in the text corpus

- No Stack Overflow samples (site unreachable during collection): consider adding a few CC BY-SA answers for common idioms, with attribution.
- No PowerPoint automation sample under a usable licence: one synthetic sample (12-07) fills the gap.
- Few public-sector business macros: item 7 is the priority for realism.
- Families 06 (reporting) and 11 (external data) are the weakest in the baseline (60 % top-2 recall): add two or three samples each.

## Rules for any addition

- Only redistributable licences (MIT, BSD, Apache-2.0, CC0, CC BY, CC BY-SA, MPL, GPL flagged). Everything else is `reference_only`.
- Every file gets the header block (source, author, licence, retrieval date, family) and a manifest entry; run `python3 -I corpus/tools/build_manifest.py` (exits non-zero on any inconsistency).
- No real personal data, no real credentials, no malware.
