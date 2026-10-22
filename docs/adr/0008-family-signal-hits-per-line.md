# ADR-0008: A family signal counts a line once

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: night agent for ticket #45 (A4), to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M3 scores a family as "the sum of matched signal weights, each signal capped by `max_hits_per_procedure`". It does not say what a hit is, and SPEC section 7 says what each matcher matches, not how many hits it makes of a line. The matchers of the engine (`engine/matchers.rs`) make one hit per use: one for each member, each call, each chain, each ProgID, each string literal and each comment. `regex` makes one per line, `procedure_name` one per procedure, `module_type` one per module.

The rules were calibrated with the prototype (`tools/rules-prototype/prototype.py`, `match_signal`). It counts a chain, a call, a regex, a string literal and a comment once per line that has one, and a member once per use. Under the engine, a line that matches a signal several times weighs as many times: `Selection.Font.Bold = True` has two members of `fmt.text-style` (`Font` and `Bold`), so one statement adds two of the three hits that the cap allows to a procedure (the prototype counts it twice as well), and two calls of `Left` on a line, in `clean.string-functions`, are two hits where the prototype has one.

A line here is the physical line where a statement starts (`CodeLine::line`): statements split from one physical line with `:` share it, and a statement continued over several physical lines is one line.

## Measures

The 74 samples of A4 (`tests/families_corpus.rs`) at the end of the series, on the main branch at 3ed608a. Samples found at top-1 / at top-2:

| rules | one hit per use | one hit per line, members per use | one hit per line (this ADR) |
| --- | --- | --- | --- |
| 0.2.0 | 51 / 66 | not measured | 52 / 65 |
| 0.3.0 | 55 / 68 | 56 / 67 | 57 / 67 |

The thresholds of A4 are 56 samples at top-1 (75 %) and 67 at top-2 (90 %). Counting lines gains one and two samples at top-1 and loses one at top-2, with the rules of 0.2.0 and of 0.3.0. A5 is the same either way, 17 of 17 signals: only the family scores count lines.

## Decisions

The scorer of the families (`engine/families.rs`) counts the hits of a signal once per line, and the cap counts lines.

- Among the hits of a signal in a procedure, those at the same place, a module and a line, count as one. The same line number in two modules is two lines. A hit that has no place, as a reference has none, counts for itself.
- It holds for every signal alike, whatever its matcher, its weight, its family or its scope. A signal of module or project scope counts the distinct lines of the module or of the project, then adds half its weight per line to each procedure. The code of a module with no procedure (ADR-0004) is counted the same way.
- The evidence follows: a score cites one item for each line it counts.
- The matchers do not change. The hits stay one per use, since the security signals and the `ratio` conditions of the traits read the same hits and the latter count uses (SPEC section 7). Only the family scores count lines.

## Alternatives rejected

- **Keep one hit per use.** The rules of 0.3.0 then find 55 of the 74 samples of A4 at top-1, 74.3 %, under the 75 % of ACCEPTANCE. Those of 0.2.0 find 51.
- **Count a line once for every matcher but `member`, as the prototype does.** The rules of 0.3.0 also meet the thresholds this way, with nothing to spare: 56 samples at top-1 (75.7 %) and 67 at top-2 (90.5 %), where all the matchers give 57 and 67. So A4 does not force the choice made here, which is a preference for one rule that holds for every signal: with the exception, the same line would weigh differently by how the author spelled a signal, `Selection.Font.Bold` being two hits as members and one as a chain. The exception stays a change of the scorer alone, with no change of the rules, if the validation prefers the parity with the prototype.
- **Count a line once for some families only**, for instance the formatting family. The weight of a signal would then depend on a family that Rust names, where SPEC wants the family logic in the YAML.
- **A key in the rules to choose, signal by signal, between lines and uses.** It needs a new key in the schema and in the core, and a choice for each signal that nothing guides: it would be set to fit samples. It can come later if a signal needs the uses.
- **Make the matchers produce one hit per line.** The security signals and the `ratio` conditions of the traits read the same hits, and SPEC section 7 has the latter count the uses of a member.

## Consequences

- `max_hits_per_procedure` now caps the lines on which a signal counts, and a rule author should read it so. The name of the key stays as SPEC M3 and the rules schema have it.
- The prototype counts a member once per use, so it still differs from the engine on a line that uses one member signal twice. The prototype is reference only.
- Statements split from one physical line with `:` share its number, so they count as one line.
