# ADR-0006: `ChrB`, and `Chr` above 127, are not folded

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: the orchestrating agent of the night run, from a finding of the agent of ticket #41, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M2 item 4 puts in the closed set of constant folding "`Chr`, `ChrW`, `ChrB` (and `$` variants) of a numeric literal". Folding feeds the `progid` and `string` matchers.

The three functions do not make the same thing:

- `ChrW(n)` makes the character of the UTF-16 code unit `n`.
- `Chr(n)` makes the character of code `n` in the code page of the machine that runs the macro. From 0 to 127 (ASCII) it is the same character on every machine; above, it depends on the machine.
- `ChrB(n)` makes a string of one byte. A VBA string holds UTF-16 code units, so one byte is half of one: `ChrB(87) & ChrB(0)` is the string `W`. Code that calls `ChrB` builds byte strings (for `MidB`, binary files, API buffers), one byte at a time.

The corpus uses `ChrB` that way: `10-06-libfiletools-bibliotheque.bas` lines 2981 to 2983 and 3103 to 3104 (`ChrB$(0)`, `ChrB$(2)`, `MidB$(hCheck, 1) = ChrB$(1)`, `ChrB$(&H22)`).

## Decisions

- `ChrB` is never folded. The statement reader knows it by name (`Folding::ChrB` in `vba/fold.rs`) only to count its calls for the `chr_obfuscation` predicate, which SPEC section 7 defines with `ChrB`.
- `Chr` is folded for codes 0 to 127. `ChrW` is folded for a UTF-16 code unit that is not a surrogate (0 to 65,535, U+D800 to U+DFFF excluded).
- A call that is not folded is unresolved, like everything outside the closed set.

This is the behaviour of `main` since the folding PR (#123). `tests/vba_fold.rs::the_bytes_of_a_code_page_and_chrb_stay_unresolved` pins it. The message of commit `dbf8e6a` says that `ChrB` folds: it is wrong, and this ADR is the record.

## Alternatives rejected

- **Fold `ChrB(n)` as the character `n`, like `Chr`.** The text would be wrong: `ChrB(87) & ChrB(0)` would give `W` followed by U+0000 instead of `W`, and the byte strings of 10-06 would become characters they do not hold.
- **Fold an even run of joined `ChrB` calls into UTF-16 code units.** A run that a variable or an odd count cuts gives half characters, and it adds a rule to the closed set for a form that no ProgID of the corpus uses. To revisit if obfuscated samples build strings that way.
- **Fold `Chr` above 127 with code page 1252.** The result would depend on a guess about the machine: a ProgID or a string matched on that guess is a false signal or a miss. ProgIDs are ASCII, so the folded range covers them.

## Consequences

- A ProgID built with `ChrB` is not resolved: `CreateObject(ChrB(87) & ChrB(0) & ...)` gives no ProgID, and no `progid` rule sees it. No sample of the corpus does it, so A5 does not change. `Facts::chr_obfuscation` still counts the calls: three joined `ChrB` calls are a mark of obfuscation.
- A `string` matcher does not see text that `Chr` builds above 127, such as an accented letter in a message.
- SPEC M2 item 4 still lists `ChrB`, as SPEC M1 step 8 still says 512 bytes after ADR-0005. Once this ADR is validated, the SPEC text can point to it.
