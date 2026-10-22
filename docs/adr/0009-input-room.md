# ADR-0009: The line budget and the room of the rule engine's input

Date: 2026-10-22 · Status: proposed (to validate) · Decided by: night agent for ticket #46 (M3 wiring, PR #139), to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M1 keeps the peak memory per file under 256 MB (ACCEPTANCE A2) and bounds each container, stream and project source. It sets no bound on what the front end makes of them: the modules a caller hands over as text may total 32 MiB (`MAX_TEXT_SIZE`), and a module may have 250,000 lines (`vba::MAX_LINES`). Before the rule engine, the front end kept nothing of a module once it had measured it, and a budget of 1,000,000 lines for all the modules of a file (`MAX_FILE_LINES`) bounded the time and the size of the report.

The rule engine changes that. It decides the families of a procedure, a module and a file (ADR-0004) and the chains of signals across modules, so it reads the whole project at once: the front end keeps the input of every module (`ModuleInput`: for each statement its text, its strings, its members, its calls and its chains) until the last module is read. That input weighs a few dozen times the text it comes from, and the lines of a file do not bound it: a module of 8 MiB made of `a.a.a.` has four million members.

## Measures

Release builds, 2026-10-21. "RSS" is the peak resident set on macOS (`/usr/bin/time -l`), "VmHWM" the peak on Linux/glibc in a Docker container (`rust:1-slim`). The modules have lines of one character, the worst case for the weight of a line, unless said otherwise.

- A line of one character is retained for about 330 bytes (macOS).
- With the budget of 1,000,000 lines, five modules of 249,999 lines (5 MB of source) peak at 481 MB RSS, above the 256 MB of A2. One such module alone: 148 MB RSS, 120 MB of live heap.
- With a budget of 100,000 lines and no room, the worst case (99,999 lines, then a module of 249,999) peaks at 204 MB RSS, 143 MB of live heap (counted with an allocator wrapper). Thirty big modules: 163 MB, so the peak does not grow with the number of modules that are refused. On Linux: one big module 154,080 kB, the worst case 180,288 kB, thirty big modules 168,252 kB.
- A budget that refuses a module after splitting it was tried: thirty refused modules peak at 861 MB RSS though the live heap peaked at 137 MB, the allocator of macOS keeping what each refused module took.
- A module of 8 MiB made of `a.a.a.` (four million members) took 220 MB to keep, before the room.
- With the room of this ADR, at the head of the next PR of the stack (#148, 16ff020), on Linux. Lines of one character: a module of 250,000 lines 176,256 kB, five modules 178,392 kB. 100,000 realistic lines: 140,984 kB in one module, 124,148 kB in four. 100,000 short lines: 92,268 kB. Ten modules of 4,900 lines: 115,968 kB. 100,000 lines of `Set o = CreateObject("Word.Application")`, all read: 134,628 kB. Statements dense in members, calls, strings or names: 49 to 80 MB. Constant-folding bombs (a module of 7 MiB, a constant of 2 MiB used ten times, a string of 5 MiB): 57 to 103 MB, the worst with four modules of them.
- The text corpus weighs 490 bytes for each line of the input it makes. A line of 88 characters and thirteen names weighs 1.5 KB.

## Decisions

The input of the engine is bounded twice, by lines and by weight.

- `MAX_FILE_LINES` goes from 1,000,000 to 100,000. The meaning stays: the modules past the budget are not read (`vba.too_many_lines`, status `too_large`), and the module that crosses it is read whole, so a file holds 100,000 lines and those of one module more, 350,000 at the most. The empty modules are still read.
- The input of the modules of a file has a room of 64 MiB (`MAX_INPUT_BYTES`), counted in what a line weighs in memory (`line_bytes`: the line, its texts, and for each fact its struct and the allocation of its name, 16 bytes beyond its length). A module takes the weight of its lines as it is adapted. If a line does not fit, the module takes nothing, is listed unread with the error `vba.too_much_code` (status `too_large`), and the modules after it may still fit. The engine then judges the file on what was read, as it does for a module that has too many lines.
- A statement is refused before its facts are read, on a ceiling that its text and its tokens give: five copies of its text, 136 bytes for each token, and for each dot or bang up to 5,120 bytes for the object of a `With` block that the reader copies into the chain, within what the reader allows a statement to copy. The tokens are dropped before the facts are read, so a hostile statement never holds both.
- Real code is not affected. The room holds 137,000 lines of the corpus, more than the budget of lines, so the lines bind first. The room binds on hostile statements: 45,000 lines of 88 characters and thirteen names fill it.

## Alternatives rejected

- **Keep 1,000,000 lines.** 481 MB for 5 MB of source, above A2.
- **Bound the bytes of source only.** The lines of a file do not bound its facts, and neither do its bytes: four million members in 8 MiB.
- **Split every module, then refuse the ones over the budget.** On macOS the allocator keeps the memory of each refused module: 861 MB for thirty of them, with no byte live. Once the budget is spent the modules after it are not split at all, so the modules ever split are those within the budget and the one that crosses it. The room refuses a module that was split, and it is bounded the same way.
- **A pessimistic ceiling.** One that counts the strings that constant folding builds from constants, and the ProgIDs that copy them, comes to about 13 times the text plus 2 KiB. It would refuse a statement of 5 MiB, a literal constant for instance, which weighs about 10 MiB, in a room of 64 MiB.
- **A room for each module.** A project of many modules would add them up, and the limit of the file is what A2 is about.
- **Options for the budget and the room.** Not now: SPEC M1 has no option for the limits of the parsers, and a constant can become an option when someone needs it.

## Consequences

- A file whose modules total more than 100,000 lines loses the modules after the one that crosses the budget, with a `too_large` error for each, and is judged on the others. The largest sample of the corpus has 3,600 lines (ACCEPTANCE A6), and I know of no real project at hand that comes near 100,000 (a hypothesis, not a measure).
- The ceiling does not count the strings that constant folding builds from constants, nor the ProgIDs that copy them. A line can weigh up to about twice its ceiling for the time it is read, but the room is charged what the line weighs once its facts are known, so what is kept stays within the room. The bombs above measure it.
- `OBJECT_BYTES`, `WITH_TEXT_COPIES` and `WITH_ALLOWANCE` restate bounds that the statement reader keeps private (`vba::facts`). A test saturates the copies of a `With` object and fails if the ceiling is below the weight, so that a change of the reader shows up. The two must change together.
- Each measure comes from one machine and one build. They show a margin of 70 to 80 MB under the limit on Linux in the worst case, and they should be repeated when a line gets a new fact, or when the allocator or the toolchain changes.
