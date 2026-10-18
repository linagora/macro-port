# ADR-0001: Framing decisions

Date: 2026-10-18 · Status: accepted · Decided by: Michel-Marie Maudet (LINAGORA)

## Context

Macro-Port is an open source analyzer of Excel VBA macros meant to support migrations from Microsoft Excel to OnlyOffice Spreadsheet, Grist and LibreOffice Calc. The framing note (French, "Note de cadrage : Macro-Port") defines four lots. Lot 1 is the deterministic inventory.

## Decisions

| Topic | Decision | Consequence |
| --- | --- | --- |
| Name | Macro-Port (repository and command `macro-port`) | Trademark (TMview, classes 9 and 42) and domain checks still pending; avoid `macro-port.com`, historically used by an unrelated US company. |
| Repository | GitHub organisation of LINAGORA | Simple start; a later transfer to a neutral body (e.g. OW2) remains possible. Keep the project neutral in tone and docs. |
| Licence | Apache-2.0 for code; CC BY 4.0 for rules and corpus metadata; corpus samples keep their original licences | SPDX header `Apache-2.0` in every source file; `cargo deny` enforces compatible dependency licences. |
| Scope of first engagement | Lot 1 strictly: extraction, metrics, security signals, family classification, JSON/HTML/CSV reports | No AI, no conversion, no target scoring until Lot 1 is accepted. |
| Platforms | CLI for Linux, Windows, macOS **and** WASM mini web app in Lot 1 | Core must be I/O-free; the web app is the last milestone (M5) and contains no analysis logic. |
| Language | English by default (code, CLI, developer docs), French translation of all user-facing strings from Lot 1 | Fluent i18n files `en` and `fr`. Pull request summaries are written in French for the reviewer. |
| Review | Michel-Marie Maudet reviews and merges | Small thematic PRs, strict CI before review, French summary at the top of each PR listing decisions to validate. |

## Consequences to watch

The web app increases Lot 1 effort; it must not delay core quality. The single reviewer is a bottleneck; CI must catch everything mechanical so that reviews focus on decisions.
