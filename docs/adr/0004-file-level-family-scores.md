# ADR-0004: Family scores of a file

Date: 2026-10-21 · Status: proposed (to validate) · Decided by: night agent for ticket #45, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M3 scores the families of each procedure. It says that the file-level `families` are the sum of the procedure scores per family, and that the file `primary_family` "uses the same tie-break". It does not say what a file with no procedure scores, what "more distinct matched signals" counts over several procedures, whether `min_score` applies to a file, or whether an unclassified procedure has secondary families.

The text corpus has 74 samples of families 01 to 13. Five of them have no procedure (02-01, 07-02, 11-01, 13-01 and 13-02): they hold module-level statements, or only `Declare` and `#If` lines. A4 judges the primary family of each file, so a file that scores nothing is wrong by construction, and the five are 6.8 points of 74. The prototype scores such a file as one pseudo-procedure `(module)` made of its lines (`analyse` in `tools/rules-prototype/prototype.py`).

## Decisions

- **Code outside the procedures.** A module with no procedure is scored once, for the file only, as if its declarations were the body of a procedure: the signals of procedure scope read them at full weight, those of module scope at half weight, each capped by `max_hits_per_procedure`. No procedure is reported for it, so there is no evidence to give. The signals of project scope do not apply: a reference describes the project, and a workbook has a document module per sheet, usually with no code, so the reference would be added once per sheet.
- **Tie-break of a file.** "Distinct matched signals" counts each signal once over the whole file, not once per procedure. A signal that matches in ten procedures is one signal.
- **Minimum score of a file.** `min_score` applies to the sum of a file as to a procedure. Below it the file is `unclassified`, and its `families` still list what scored.
- **Secondary families.** A procedure that is `unclassified` has none, since there is no primary score to take a ratio of. A file has none, because the schema has no field for them.
- **Ratio.** A family is secondary when its score is at least `secondary_ratio` times the primary score, within 1e-9: 0.55 x 50 is 27.500000000000004 in binary floating point, and the score of 27.5 is at the ratio.
- **Schema.** The description of the file-level `families` read "Aggregated over procedures (sum of scores)". It now adds "and over the code of modules that have no procedure", in `schema/report.schema.json` and in `macroport_core::report`. The text changes and the structure does not, so `schema_version` stays.

## Alternatives rejected

- **Report a pseudo-procedure `(module)`**, as the prototype does. It puts in the report a procedure that the source does not have, and it would count in the procedures of the file and in its metrics.
- **Leave those modules out**, which is SPEC M3 to the letter. The five samples would be `unclassified`, and A4 would miss its thresholds by that much alone.
- **Apply the signals of project scope to those modules.** The family of a file would follow the number of its empty sheets.
- **Add the distinct signals of each procedure** for the tie-break of a file. Two families with the same total would be ranked by the number of procedures that match, so the winner would depend on how the code is cut into procedures.
- **Give an unclassified procedure secondary families, relative to its best score.** It would name families for a procedure that the rules do not classify.

## Consequences

- Any module with no procedure counts once, in any project. An empty UserForm adds half the weight of `form.userform-module` (2.5) to 05-forms. Under the shipped rules a document module with no code adds nothing: none of its module-scope signals matches it.
- The declarations of a module feed its procedure-scope signals only when it has no procedure, so adding a procedure to a module of statements changes how the same lines count. The prototype does the same.
- The rule relies on the conversion of the parser output to `ProjectInput` (ticket #43) putting the logical lines outside any procedure in `ModuleInput::declarations`, as its documentation says. If it does not, the five samples stay `unclassified` and the A4 test shows it.
